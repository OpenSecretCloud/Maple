//! Minimal XTEST input injector for the Xvfb test display.
//! Usage:
//!   xsend_input click <x> <y>              root-coords click
//!   xsend_input type <x> <y> <text...>     click into place, then type text
//! ASCII only; Enter is sent as the "Return" keysym when text contains
//! the escape `\n` at the end.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, Keysym};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let display = std::env::var("DISPLAY")?;
    let (conn, _) = RustConnection::connect(Some(&display))?;
    let xtest = conn.xtest_get_version(2, 2)?.reply()?;
    assert!(xtest.major_version >= 2, "XTEST 2 unavailable");

    // Without a window manager, X input focus never moves on its own.
    // Explicitly focus the "Maple" window so keyboard events deliver.
    let maple = find_maple_window(&conn)?;
    conn.set_input_focus(
        x11rb::protocol::xproto::InputFocus::POINTER_ROOT,
        maple,
        0_u32, // CurrentTime
    )?;
    conn.flush()?;

    match args[1].as_str() {
        "click" => {
            let x: i16 = args[2].parse()?;
            let y: i16 = args[3].parse()?;
            fake_motion(&conn, x, y)?;
            click(&conn)?;
        }
        "type" => {
            let x: i16 = args[2].parse()?;
            let y: i16 = args[3].parse()?;
            let text = args[4..].join(" ");
            fake_motion(&conn, x, y)?;
            click(&conn)?;
            std::thread::sleep(std::time::Duration::from_millis(120));
            for character in text.chars() {
                if character == '\n' {
                    press(&conn, 0xff0d /* Return */, false)?;
                    continue;
                }
                let (keysym, shifted) =
                    keysym_for(character).ok_or_else(|| format!("no keysym for {character:?}"))?;
                press(&conn, keysym, shifted)?;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
        "hotkey" => {
            // hotkey <ctrl|alt|shift>-<key>  e.g. ctrl-q
            let combo = args[2].clone();
            let (modifier, key) = combo.split_once('-').ok_or("expected <modifier>-<key>")?;
            let modifier_keysym: Keysym = match modifier {
                "ctrl" | "control" => 0xffe3,
                "alt" => 0xffe9,
                "shift" => 0xffe1,
                other => return Err(format!("unsupported modifier {other:?}").into()),
            };
            let key_char = key.chars().next().ok_or("missing key")?;
            let (keysym, _) =
                keysym_for(key_char).ok_or_else(|| format!("no keysym for {key:?}"))?;
            press(&conn, modifier_keysym, false)?;
            std::thread::sleep(std::time::Duration::from_millis(30));
            press(&conn, keysym, false)?;
            std::thread::sleep(std::time::Duration::from_millis(40));
            release(&conn, keysym)?;
            release(&conn, modifier_keysym)?;
        }
        other => return Err(format!("unknown verb {other:?}").into()),
    }
    conn.get_input_focus()?.reply()?;
    Ok(())
}

fn find_maple_window(
    conn: &RustConnection,
) -> Result<x11rb::protocol::xproto::Window, Box<dyn std::error::Error>> {
    use x11rb::protocol::xproto::ConnectionExt as _;
    let setup = conn.setup();
    let root = setup.roots[0].root;
    let tree = conn.query_tree(root)?.reply()?;
    for window in tree.children {
        let name = conn
            .get_property(
                false,
                window,
                x11rb::protocol::xproto::AtomEnum::WM_NAME,
                x11rb::protocol::xproto::AtomEnum::STRING,
                0,
                64,
            )?
            .reply()?;
        let bytes = name.value;
        if bytes.as_slice() == b"Maple" {
            return Ok(window);
        }
    }
    Err("Maple window not found".into())
}

fn release(conn: &RustConnection, keysym: Keysym) -> Result<(), x11rb::errors::ReplyError> {
    let mapping = conn.get_keyboard_mapping(8, 248)?.reply()?;
    let mut keycode = 0;
    'outer: for (index, chunk) in mapping
        .keysyms
        .chunks(mapping.keysyms_per_keycode as usize)
        .enumerate()
    {
        for sym in chunk.iter() {
            if *sym != 0 && *sym == keysym {
                keycode = (index + 8) as u8;
                break 'outer;
            }
        }
    }
    if keycode == 0 {
        return Ok(());
    }
    conn.xtest_fake_input(3, keycode, 0, 0, 0, 0, 0)?;
    conn.flush()?;
    Ok(())
}

fn fake_motion(conn: &RustConnection, x: i16, y: i16) -> Result<(), x11rb::errors::ReplyError> {
    conn.xtest_fake_input(6, 0, 0, 0, x, y, 0)?;
    conn.flush()?;
    Ok(())
}

fn click(conn: &RustConnection) -> Result<(), x11rb::errors::ReplyError> {
    conn.xtest_fake_input(4, 1, 0, 0, 0, 0, 0)?;
    conn.flush()?;
    std::thread::sleep(std::time::Duration::from_millis(60));
    conn.xtest_fake_input(5, 1, 0, 0, 0, 0, 0)?;
    conn.flush()?;
    Ok(())
}

fn press(
    conn: &RustConnection,
    keysym: Keysym,
    shifted: bool,
) -> Result<(), x11rb::errors::ReplyError> {
    // Map keysym to keycode via the server's keyboard mapping.
    let mapping = conn.get_keyboard_mapping(8, 248)?.reply()?;
    let mut keycode = 0;
    let mut column = 0;
    'outer: for (index, chunk) in mapping
        .keysyms
        .chunks(mapping.keysyms_per_keycode as usize)
        .enumerate()
    {
        for (col, sym) in chunk.iter().enumerate() {
            if *sym != 0 && *sym == keysym {
                keycode = (index + 8) as u8;
                column = col;
                break 'outer;
            }
        }
    }
    if keycode == 0 {
        panic!("keysym {keysym:#x} not in keyboard map");
    }
    let shift_code: u8 = 50; // left shift
    let _ = column;
    if shifted {
        conn.xtest_fake_input(2, shift_code, 0, 0, 0, 0, 0)?;
    }
    conn.xtest_fake_input(2, keycode, 0, 0, 0, 0, 0)?;
    conn.flush()?;
    std::thread::sleep(std::time::Duration::from_millis(40));
    conn.xtest_fake_input(3, keycode, 0, 0, 0, 0, 0)?;
    if shifted {
        conn.xtest_fake_input(3, shift_code, 0, 0, 0, 0, 0)?;
    }
    conn.flush()?;
    Ok(())
}

fn keysym_for(character: char) -> Option<(Keysym, bool)> {
    let code = character as u32;
    match character {
        '\t' => Some((0xff09, false)),
        'a'..='z' | '0'..='9' => Some((code, false)),
        'A'..='Z' => Some((character.to_ascii_lowercase() as u32, true)),
        ' ' | '.' | '-' | '_' | '/' | '@' | ':' | '!' | '?' | '#' | '%' | '+' | '(' | ')' => {
            let shifted = matches!(character, '_' | '?' | '%' | '+' | '(' | '!' | ':' | '@');
            let base = match character {
                '_' => '-',
                '?' => '/',
                '%' => '5',
                '+' => '=',
                '(' => '9',
                ')' => '0',
                '!' => '1',
                ':' => ';',
                '@' => '2',
                other => other,
            };
            Some((base as u32, shifted))
        }
        _ => (code < 0x80).then_some((code, false)),
    }
}
