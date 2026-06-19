//! Tauri command surface. These mirror the function names in `src/lib/api.ts`.

use awflash_core::model::{Catalog, DriveInfo, FlashRequest, LocalImage};
use awflash_core::{catalog, device, enumerate, inspect, sshkeys};
use tauri::{AppHandle, State};

use crate::orchestrator;
use crate::state::SharedState;

#[tauri::command]
pub async fn fetch_catalog(url: String) -> Result<Catalog, String> {
    catalog::fetch_catalog(&url).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_drives() -> Result<Vec<DriveInfo>, String> {
    enumerate::list_drives().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn inspect_image(path: String) -> Result<LocalImage, String> {
    inspect::inspect_image(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn detect_ssh_keys() -> Vec<String> {
    sshkeys::detect_ssh_keys()
}

#[tauri::command]
pub fn eject_drive(device: String) -> Result<(), String> {
    device::eject_path(&device);
    Ok(())
}

#[tauri::command]
pub fn start_flash(
    app: AppHandle,
    state: State<'_, SharedState>,
    request: FlashRequest,
) -> Result<(), String> {
    orchestrator::start(app, &state, request)
}

#[tauri::command]
pub fn cancel_flash(state: State<'_, SharedState>) -> Result<(), String> {
    orchestrator::cancel(&state)
}
