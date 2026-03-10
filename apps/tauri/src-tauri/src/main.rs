//! HyprDrive Tauri Backend
//!
//! This is a THIN SHELL — zero core logic.
//! Manages the native window and system tray.
//! All data comes from hyprdrive-daemon via WebSocket.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
