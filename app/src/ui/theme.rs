//! Maple theme tokens for the gpui frontend.
//!
//! Dark values are measured from the Tauri app's dark theme and light
//! values from its light theme (see docs/maple-theme-spec.md). Each token
//! is a function that reads the active palette, so a theme switch needs
//! only a window refresh. Values are `u32` hex literals for `gpui::rgb`.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Theme preference from settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preference {
    System,
    Dark,
    Light,
}

impl Preference {
    pub fn parse(value: &str) -> Self {
        match value {
            "dark" => Self::Dark,
            "light" => Self::Light,
            _ => Self::System,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }

    /// The next choice in the settings cycle.
    pub fn next(self) -> Self {
        match self {
            Self::System => Self::Dark,
            Self::Dark => Self::Light,
            Self::Light => Self::System,
        }
    }
}

static PREFERENCE: AtomicU8 = AtomicU8::new(0);
static LIGHT_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_preference(preference: Preference) {
    PREFERENCE.store(
        match preference {
            Preference::System => 0,
            Preference::Dark => 1,
            Preference::Light => 2,
        },
        Ordering::Relaxed,
    );
}

pub fn preference() -> Preference {
    match PREFERENCE.load(Ordering::Relaxed) {
        1 => Preference::Dark,
        2 => Preference::Light,
        _ => Preference::System,
    }
}

/// Pick the palette for the preference and the window appearance. Returns
/// whether the active palette changed, in which case every view must
/// render again.
pub fn resolve(appearance: gpui::WindowAppearance) -> bool {
    let light = match preference() {
        Preference::Dark => false,
        Preference::Light => true,
        Preference::System => matches!(
            appearance,
            gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
        ),
    };
    LIGHT_ACTIVE.swap(light, Ordering::Relaxed) != light
}

pub fn is_light() -> bool {
    LIGHT_ACTIVE.load(Ordering::Relaxed)
}

macro_rules! tokens {
    ($($(#[$doc:meta])* $name:ident: $dark:literal, $light:literal;)*) => {
        struct Palette {
            $($name: u32,)*
        }

        const DARK: Palette = Palette { $($name: $dark,)* };
        const LIGHT: Palette = Palette { $($name: $light,)* };

        $(
            $(#[$doc])*
            pub fn $name() -> u32 {
                palette().$name
            }
        )*
    };
}

fn palette() -> &'static Palette {
    if is_light() { &LIGHT } else { &DARK }
}

tokens! {
    bg_app: 0x0a0a0a, 0xfafafa;
    bg_sidebar: 0x262626, 0xf5f5f5;
    bg_elevated: 0x171717, 0xffffff;
    bg_sidebar_card: 0x303030, 0xffffff;
    bg_sidebar_pill: 0x1c1c1c, 0xe5e5e5;
    bg_input: 0x121212, 0xffffff;
    bg_user_bubble: 0x171717, 0xf5f5f5;
    bg_code_block: 0x0d0d0d, 0xf3f3f3;
    bg_tool_card: 0x121212, 0xf7f7f7;

    border: 0x2f2f37, 0xe8e8ed;
    border_subtle: 0x232323, 0xeeeeee;

    text_primary: 0xfafafa, 0x262626;
    text_secondary: 0xa3a3a3, 0x515151;
    text_muted: 0x666666, 0x737373;
    text_faint: 0x4a4a4a, 0xa8a8a8;

    /// Maple coral: send button, focus, caret, permission prompts.
    accent: 0xff9771, 0xff9771;
    accent_hover: 0xe77040, 0xe77040;

    status_running: 0xa3a3a3, 0x737373;
    status_success: 0x87a253, 0x6d8a3a;
    status_error: 0xd05e41, 0xc4503a;
    /// Wavy underline under a misspelled word in the composer.
    spell_error: 0xe0553f, 0xd0402a;
    status_warning: 0xce994b, 0xb8832f;

    code_text: 0xe8e8e8, 0x262626;
    link: 0xb7b7b7, 0x616161;

    /// Permission card fill/border from the Maple spec.
    permission_fill: 0x191210, 0xfaf4f1;
    permission_border: 0x784a38, 0xfdc8b8;

    user_bubble_border: 0x262626, 0xe5e5e5;

    /// Sidebar chrome: segmented toggle track, row hover, selected row.
    bg_sidebar_chrome: 0x1c1c1c, 0xffffff;
    bg_sidebar_row_hover: 0x404040, 0xe8e8e8;
    bg_sidebar_row_selected: 0x525252, 0xdedede;

    /// Empty-state heading. The web app draws a gradient from #c3c3cb to
    /// #9d6c5f; gpui text has no gradient, so this is the visual midpoint.
    display_text: 0xc9b3a6, 0x7a5a4e;

    /// Send button gradient stops (Maple coral to a darker coral).
    send_top: 0xff9771, 0xff9771;
    send_bottom: 0xe36e47, 0xe36e47;

    /// Title bar control buttons.
    bg_title_control: 0x3a3a3a, 0xe5e5e5;
    bg_title_control_hover: 0x4a4a4a, 0xd4d4d4;

    /// Text input caret; near the primary text color of each palette.
    text_cursor: 0xe7e7ea, 0x262626;
}

/// Translucent selection highlight in text inputs. Blue on both palettes,
/// a little stronger on light where the white field washes it out.
pub fn text_selection() -> gpui::Rgba {
    if is_light() {
        gpui::rgba(0x4a7dff4d)
    } else {
        gpui::rgba(0x4a7dff40)
    }
}

/// Translucent fill for hover states and inline code over any surface.
pub fn overlay_hover() -> gpui::Hsla {
    if is_light() {
        gpui::hsla(0., 0., 0., 0.06)
    } else {
        gpui::hsla(0., 0., 1., 0.08)
    }
}

/// Scrollbar thumb over the transcript.
pub fn scrollbar_thumb() -> gpui::Rgba {
    if is_light() {
        gpui::rgba(0x00000033)
    } else {
        gpui::rgba(0xffffff26)
    }
}

/// Placeholder text in inputs.
pub fn placeholder() -> gpui::Hsla {
    if is_light() {
        gpui::hsla(0., 0., 0., 0.35)
    } else {
        gpui::hsla(0., 0., 1., 0.3)
    }
}
