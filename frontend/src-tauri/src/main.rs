// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // SAFETY: this is the process entry point and runs before Maple, Tauri, or
    // the async runtime can create another thread that might read the process
    // environment.
    unsafe { app_lib::run() };
}
