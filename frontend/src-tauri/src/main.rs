// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // SAFETY: this is Maple's first operation, before Tauri, Tokio, logging,
    // ACP, plugins, or any application-owned thread can read the environment.
    unsafe { app_lib::run() };
}
