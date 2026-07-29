// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("acp") {
        if let Err(error) = app_lib::run_acp_connector() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    app_lib::run();
}
