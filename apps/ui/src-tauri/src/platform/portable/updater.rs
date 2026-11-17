//! Native previews are updated manually; Flatpak updates belong to the package manager.
use tauri::{AppHandle, State};

#[derive(Default)]
pub struct UpdateState;

#[derive(serde::Serialize)]
pub struct AvailableUpdate {
    version: String,
    notes: Option<String>,
}

#[derive(Clone, serde::Serialize)]
pub struct UpdateProgress;

#[tauri::command]
pub async fn check_for_updates(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<Option<AvailableUpdate>, String> {
    let _ = (app, state);
    Err("Update this preview through the download page or your package manager.".into())
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
    on_progress: tauri::ipc::Channel<UpdateProgress>,
) -> Result<(), String> {
    let _ = (app, state, on_progress);
    Err("Update this preview through the download page or your package manager.".into())
}

pub fn resume_after_update(_: &AppHandle) {}
