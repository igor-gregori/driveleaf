// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod spike;

fn main() {
    tauri::Builder::default()
        .manage(spike::SpikeState::default())
        .invoke_handler(tauri::generate_handler![
            spike::spike_mode,
            spike::spike_prepare,
            spike::spike_fetch,
            spike::spike_report,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
