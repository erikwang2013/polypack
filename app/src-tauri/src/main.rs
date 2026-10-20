//! Minimal Tauri v2 shell for the PolyPack build-history view (T-M9-18).
//!
//! The frontend is a committed static HTML/CSS/JS bundle; the only backend
//! surface is the read-only `history_records` command in [`history`].

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod history;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![history::history_records])
        .run(tauri::generate_context!())
        .expect("poly-gui: Tauri runtime failed");
}
