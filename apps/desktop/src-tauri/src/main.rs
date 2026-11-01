// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use tauri::Manager;
use tauri::window::{Effect, EffectsBuilder};

fn main() {
    tauri::Builder::default()
        .manage(commands::AppState::default())
        .setup(|app| {
            if driveleaf_win::system::supports_mica()
                && let Some(window) = app.get_webview_window("main")
            {
                // Only a finishing touch, so a failure just leaves the plain background.
                let _ = window.set_effects(EffectsBuilder::new().effect(Effect::Mica).build());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_volumes,
            commands::app_info,
            commands::start_scan,
            commands::cancel_scan,
            commands::get_node,
            commands::get_ancestry,
            commands::get_path,
            commands::count_other_links,
            commands::get_children,
            commands::get_treemap,
            commands::top_files,
            commands::file_types,
            commands::search,
            commands::reveal_in_explorer,
            commands::show_properties,
            commands::recycle,
            commands::relaunch_elevated,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
