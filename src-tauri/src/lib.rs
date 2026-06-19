mod commands;
mod orchestrator;
mod state;

use state::RunState;
use std::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(RunState::default()))
        .setup(|app| {
            // Frameless custom chrome on Windows/Linux; macOS keeps native
            // traffic lights via the Overlay titleBarStyle (set in tauri.conf.json).
            #[cfg(not(target_os = "macos"))]
            {
                use tauri::Manager;
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_decorations(false);
                }
            }
            let _ = app;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::fetch_catalog,
            commands::list_drives,
            commands::inspect_image,
            commands::detect_ssh_keys,
            commands::eject_drive,
            commands::start_flash,
            commands::cancel_flash,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
