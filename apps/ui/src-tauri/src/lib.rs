//! Tauri backend exposing Encore recorder IPC, config/autostart, clip library, waveforms, and editor commands.
mod tray;
mod lifecycle;
#[cfg(windows)]
#[path = "platform/windows/updater.rs"]
mod updater;
#[cfg(not(windows))]
#[path = "platform/portable/updater.rs"]
mod updater;
#[cfg(windows)]
#[path = "platform/windows/game_picker.rs"]
mod game_picker;
#[cfg(not(windows))]
#[path = "platform/portable/game_picker.rs"]
mod game_picker;

use encore_common::platform::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use encore_clipstore::{ClipQuery, ClipRecord, ClipStore};
use encore_common::autostart;
use encore_common::ipc::{self, AudioDeviceInfo, IpcRequest, IpcResponse, RecorderStatus};
use encore_common::{ffmpeg_path, Config};
use encore_editor::{
    build_montage, copy_file_to_clipboard, export_share_clip, export_vertical_9_16, trim_clip, TrimOpts,
    VerticalExportOpts,
};
use tauri::{AppHandle, Manager, State};


pub struct AppState {
    pub store: Arc<Mutex<ClipStore>>,
}

impl AppState {
    pub fn open() -> Result<Self, String> {
        let store = ClipStore::open_default()?;
        Ok(Self { store: Arc::new(Mutex::new(store)) })
    }
}

fn find_recorder_exe(resource_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(res) = resource_dir {
        let cand = res.join("encore-recorder.exe");
        if cand.exists() {
            return Some(cand);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        for anc in exe.ancestors().skip(1) {
            for rel in [
                "encore-recorder.exe",
                "resources\\encore-recorder.exe",
                "target\\release\\encore-recorder.exe",
                "target\\debug\\encore-recorder.exe",
            ] {
                let cand = anc.join(rel);
                if cand.exists() {
                    return Some(cand);
                }
            }
        }
    }
    None
}

fn unique_output_path(dir: &Path, stem: &str, suffix: &str) -> PathBuf {
    let first = dir.join(format!("{stem}{suffix}.mp4"));
    if !first.exists() {
        return first;
    }
    for n in 2..=999 {
        let cand = dir.join(format!("{stem}{suffix}_{n}.mp4"));
        if !cand.exists() {
            return cand;
        }
    }
    dir.join(format!("{stem}{suffix}_{}.mp4", now_unix()))
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn is_newer(a: &Path, than: &Path) -> bool {
    let ma = std::fs::metadata(a).and_then(|m| m.modified()).ok();
    let mb = std::fs::metadata(than).and_then(|m| m.modified()).ok();
    match (ma, mb) {
        (Some(ta), Some(tb)) => ta >= tb,
        _ => false,
    }
}

/// Lazily stream-copy individual AAC tracks into `.m4a` files so WebView2 can live-mute multi-track clips.
pub fn extract_track_audio_files(
    ffmpeg: &Path,
    clip_id: &str,
    src: &Path,
    tracks: u32,
    cache_dir: &Path,
) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
    let count = tracks.min(3);
    let mut out = Vec::with_capacity(count as usize);
    for idx in 0..count {
        let dst = cache_dir.join(format!("{clip_id}_t{idx}.m4a"));
        let valid = dst.exists()
            && std::fs::metadata(&dst).map(|m| m.len() > 0).unwrap_or(false)
            && is_newer(&dst, src);
        if !valid {
            let map_arg = format!("0:a:{idx}");
            let st = Command::new(ffmpeg)
                .args(["-y", "-v", "error", "-i"])
                .arg(src)
                .args(["-map", &map_arg, "-vn", "-c:a", "copy"])
                .arg(&dst)
                .background()
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| format!("ffmpeg extract track {idx}: {e}"))?;
            if !st.success() {
                return Err(format!("ffmpeg failed extracting audio track {idx}"));
            }
        }
        out.push(dst.to_string_lossy().into_owned());
    }
    Ok(out)
}

async fn run_blocking<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    let guard = lifecycle::global().begin_task()?;
    tauri::async_runtime::spawn_blocking(move || { let _guard = guard; f() })
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

/// Explicit exports finish before the library exits. Read-only polling is not an exit blocker.
async fn run_export<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    let guard = lifecycle::global().begin_export()?;
    tauri::async_runtime::spawn_blocking(move || { let _guard = guard; f() })
        .await.map_err(|e| format!("export task join error: {e}"))?
}

#[tauri::command]
async fn get_recorder_status() -> Option<RecorderStatus> {
    run_blocking(|| match ipc::send_request(&IpcRequest::GetStatus) {
        Ok(IpcResponse::Status(s)) => Ok(s),
        Ok(IpcResponse::Err(e)) => Err(e),
        _ => Err("offline".into()),
    })
    .await
    .ok()
}

pub fn launch_recorder_blocking(res_dir: Option<&Path>) -> Result<(), String> {
    if !encore_common::platform::capabilities().recording {
        return Err(encore_common::platform::RECORDING_UNAVAILABLE.into());
    }
    if matches!(ipc::send_request(&IpcRequest::GetStatus), Ok(IpcResponse::Status(_))) {
        let _ = ipc::send_request(&IpcRequest::SetTrayVisible(true));
        return Ok(());
    }
    let exe = find_recorder_exe(res_dir)
        .ok_or_else(|| "Could not find encore-recorder.exe".to_string())?;
    let mut child = Command::new(&exe)
        .background()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start {}: {e}", exe.display()))?;
    for _ in 0..30 {
        if matches!(ipc::send_request(&IpcRequest::GetStatus), Ok(IpcResponse::Status(_))) {
            return Ok(());
        }
        if let Ok(Some(exit_status)) = child.try_wait() {
            return Err(format!(
                "Recorder exited immediately (status {exit_status}). Check your hotkey or settings."
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err("Recorder launched but did not respond on IPC pipe".into())
}

#[tauri::command]
async fn start_recorder(app: AppHandle) -> Result<(), String> {
    let _work = lifecycle::global().begin_export()?;
    let res_dir = app.path().resource_dir().ok();
    run_blocking(move || launch_recorder_blocking(res_dir.as_deref())).await
}

#[tauri::command]
async fn stop_recorder() -> Result<(), String> {
    run_blocking(|| match ipc::send_request(&IpcRequest::Shutdown) {
        Ok(IpcResponse::Ok) => Ok(()),
        Ok(IpcResponse::Err(e)) => Err(e),
        Err(_) => Ok(()), // Already offline
        _ => Err("unexpected response".into()),
    })
    .await
}

#[tauri::command]
async fn trigger_save_clip() -> Result<(), String> {
    let _work = lifecycle::global().begin_export()?;
    run_blocking(|| match ipc::send_request(&IpcRequest::SaveClip) {
        Ok(IpcResponse::Ok) => Ok(()),
        Ok(IpcResponse::Err(e)) => Err(e),
        Err(e) => Err(format!("Recorder is offline: {e}")),
        _ => Err("unexpected response".into()),
    })
    .await
}

#[tauri::command]
async fn list_audio_devices() -> Vec<AudioDeviceInfo> {
    run_blocking(|| match ipc::send_request(&IpcRequest::ListAudioDevices) {
        Ok(IpcResponse::AudioDevices(devs)) => Ok(devs),
        _ => Ok(ipc::list_audio_devices()),
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
async fn list_audio_applications() -> Result<encore_common::processes::ApplicationAudioOptions, String> {
    run_blocking(|| Ok(encore_common::processes::application_audio_options())).await
}

#[tauri::command]
async fn list_capture_applications() -> Result<Vec<game_picker::CaptureApplication>, String> {
    run_blocking(game_picker::running_applications).await
}

#[tauri::command]
async fn pick_game_executable() -> Result<Option<String>, String> {
    run_blocking(|| {
        rfd::FileDialog::new().set_title("Choose your game executable")
            .add_filter("Windows game executable", &["exe"]).pick_file()
            .map(|path| game_picker::executable_filename(&path)).transpose()
    }).await
}

#[tauri::command]
fn get_config() -> Config {
    let mut cfg = Config::load();
    cfg.start_with_windows = encore_common::platform::capabilities().autostart
        && (cfg.start_with_windows || autostart::is_autostart_enabled());
    cfg
}

#[tauri::command]
async fn save_config(app: AppHandle, cfg: Config) -> Result<(), String> {
    if encore_common::platform::capabilities().recording {
        encore_common::validate_hotkey(&cfg.hotkey)?;
        cfg.validate_audio()?;
    }
    let res_dir = app.path().resource_dir().ok();
    run_blocking(move || {
        let prev_cfg = Config::load();
        std::fs::create_dir_all(&cfg.clips_dir).map_err(|e| e.to_string())?;
        cfg.save().map_err(|e| e.to_string())?;
        let target_exe = find_recorder_exe(res_dir.as_deref())
            .or_else(|| std::env::current_exe().ok());
        if let Some(exe) = target_exe.filter(|_| encore_common::platform::capabilities().autostart) {
            autostart::set_autostart(&exe, cfg.start_with_windows)
                .map_err(|e| format!("Failed updating startup setting: {e}"))?;
        }
        match ipc::send_request(&IpcRequest::ReloadConfig) {
            Ok(IpcResponse::Ok) | Err(_) => Ok(()),
            Ok(IpcResponse::Err(e)) => {
                let _ = prev_cfg.save();
                Err(e)
            }
            _ => Ok(()),
        }
    })
    .await
}

#[tauri::command]
async fn pick_clips_folder(current: Option<String>) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dlg = rfd::FileDialog::new().set_title("Select Encore Clips Folder");
        if let Some(ref cur) = current {
            let p = Path::new(cur);
            if p.exists() {
                dlg = dlg.set_directory(p);
            }
        }
        dlg.pick_folder().map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
async fn sync_and_list_clips(state: State<'_, AppState>, query: ClipQuery) -> Result<Vec<ClipRecord>, String> {
    let store = state.store.clone();
    run_blocking(move || {
        let cfg = Config::load();
        let ff = ffmpeg_path();
        let guard = store.lock().map_err(|_| "store mutex poisoned".to_string())?;
        guard.sync_dir(&cfg.clips_dir, &ff)?;
        guard.list(&query)
    })
    .await
}

#[tauri::command]
async fn toggle_favorite(state: State<'_, AppState>, id: String, favorite: bool) -> Result<(), String> {
    let store = state.store.clone();
    run_blocking(move || {
        store.lock().map_err(|_| "store mutex poisoned".to_string())?.set_favorite(&id, favorite)
    })
    .await
}

#[tauri::command]
async fn update_clip_details(
    state: State<'_, AppState>,
    id: String,
    title: String,
    tag: String,
    description: String,
) -> Result<(), String> {
    let store = state.store.clone();
    run_blocking(move || {
        store
            .lock()
            .map_err(|_| "store mutex poisoned".to_string())?
            .update_details(&id, &title, &tag, &description)
    })
    .await
}

#[tauri::command]
async fn delete_clip(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let store = state.store.clone();
    run_blocking(move || {
        store.lock().map_err(|_| "store mutex poisoned".to_string())?.delete(&id)
    })
    .await
}

#[tauri::command]
async fn get_clip_waveforms(state: State<'_, AppState>, id: String) -> Result<Vec<Vec<f32>>, String> {
    let store = state.store.clone();
    run_blocking(move || {
        let ff = ffmpeg_path();
        store.lock().map_err(|_| "store mutex poisoned".to_string())?.get_waveforms(&id, &ff)
    })
    .await
}

#[tauri::command]
async fn get_clip_audio_tracks(state: State<'_, AppState>, id: String) -> Result<Vec<String>, String> {
    let store = state.store.clone();
    run_blocking(move || {
        let rec = store.lock().map_err(|_| "store mutex poisoned".to_string())?.get(&id)?;
        if rec.audio_tracks <= 1 {
            return Ok(Vec::new());
        }
        let cache = Config::dir().join("waveforms");
        let ff = ffmpeg_path();
        extract_track_audio_files(&ff, &rec.id, Path::new(&rec.path), rec.audio_tracks, &cache)
    })
    .await
}

#[tauri::command]
async fn trim_clip_cmd(
    state: State<'_, AppState>,
    id: String,
    start_secs: f64,
    end_secs: f64,
    muted_tracks: Vec<bool>,
    track_volumes: Option<Vec<f64>>,
) -> Result<ClipRecord, String> {
    let store = state.store.clone();
    let volumes = track_volumes.unwrap_or_default();
    run_export(move || {
        let rec = store.lock().map_err(|_| "store mutex poisoned".to_string())?.get(&id)?;
        let src = PathBuf::from(&rec.path);
        let dir = src.parent().ok_or("invalid clip path")?;
        let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("Clip");
        let dst = unique_output_path(dir, stem, "_trim");
        let ff = ffmpeg_path();
        trim_clip(
            &ff,
            &src,
            &dst,
            &TrimOpts {
                start_secs,
                end_secs,
                audio_tracks: rec.audio_tracks,
                muted_tracks: &muted_tracks,
                track_volumes: &volumes,
            },
        )?;

        let cfg = Config::load();
        let guard = store.lock().map_err(|_| "store mutex poisoned".to_string())?;
        guard.sync_dir(&cfg.clips_dir, &ff)?;
        let created = guard
            .list(&ClipQuery::default())?
            .into_iter()
            .find(|c| c.path == dst.to_string_lossy())
            .ok_or_else(|| "Trimmed clip created but not indexed".to_string())?;
        let _ = guard.update_details(&created.id, &format!("{} (Trim)", rec.title), &rec.tag, &rec.description);
        guard.get(&created.id)
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Keep the existing flat Tauri command arguments compatible.
async fn export_vertical_cmd(
    state: State<'_, AppState>,
    id: String,
    blur_bg: bool,
    crop: Option<[f64; 4]>,
    start_secs: Option<f64>,
    end_secs: Option<f64>,
    muted_tracks: Vec<bool>,
    track_volumes: Option<Vec<f64>>,
) -> Result<ClipRecord, String> {
    let store = state.store.clone();
    let volumes = track_volumes.unwrap_or_default();
    run_export(move || {
        let rec = store.lock().map_err(|_| "store mutex poisoned".to_string())?.get(&id)?;
        let src = PathBuf::from(&rec.path);
        let dir = src.parent().ok_or("invalid clip path")?;
        let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("Clip");
        let dst = unique_output_path(dir, stem, if crop.is_some() { "_crop" } else { "_9x16" });
        let ff = ffmpeg_path();
        export_vertical_9_16(
            &ff,
            &src,
            &dst,
            &VerticalExportOpts {
                blur_bg,
                crop,
                start_secs,
                end_secs,
                audio_tracks: rec.audio_tracks,
                muted_tracks: &muted_tracks,
                track_volumes: &volumes,
            },
        )?;

        let cfg = Config::load();
        let guard = store.lock().map_err(|_| "store mutex poisoned".to_string())?;
        guard.sync_dir(&cfg.clips_dir, &ff)?;
        let created = guard
            .list(&ClipQuery::default())?
            .into_iter()
            .find(|c| c.path == dst.to_string_lossy())
            .ok_or_else(|| "Vertical export created but not indexed".to_string())?;
        let _ = guard.update_details(&created.id, &format!("{} ({})", rec.title, if crop.is_some() { "Crop" } else { "9:16" }), &rec.tag, &rec.description);
        guard.get(&created.id)
    })
    .await
}

#[tauri::command]
async fn build_montage_cmd(
    state: State<'_, AppState>,
    ids: Vec<String>,
    title: String,
) -> Result<ClipRecord, String> {
    let store = state.store.clone();
    run_export(move || {
        let inputs: Vec<PathBuf> = {
            let guard = store.lock().map_err(|_| "store mutex poisoned".to_string())?;
            ids.iter()
                .map(|id| guard.get(id).map(|r| PathBuf::from(r.path)))
                .collect::<Result<Vec<_>, _>>()?
        };
        let cfg = Config::load();
        let stem = format!("Encore_Montage_{}", now_unix());
        let dst = unique_output_path(&cfg.clips_dir, &stem, "");
        let ff = ffmpeg_path();
        build_montage(&ff, &inputs, &dst)?;

        let guard = store.lock().map_err(|_| "store mutex poisoned".to_string())?;
        guard.sync_dir(&cfg.clips_dir, &ff)?;
        let created = guard
            .list(&ClipQuery::default())?
            .into_iter()
            .find(|c| c.path == dst.to_string_lossy())
            .ok_or_else(|| "Montage created but not indexed".to_string())?;
        let final_title = if title.trim().is_empty() { "Highlight Montage" } else { title.trim() };
        let _ = guard.update_details(&created.id, final_title, "Montage", "");
        guard.get(&created.id)
    })
    .await
}

#[tauri::command]
async fn copy_clip_to_clipboard(
    state: State<'_, AppState>,
    id: String,
    start_secs: Option<f64>,
    end_secs: Option<f64>,
    muted_tracks: Option<Vec<bool>>,
    track_volumes: Option<Vec<f64>>,
) -> Result<(), String> {
    let store = state.store.clone();
    run_export(move || {
        let rec = store.lock().map_err(|_| "store mutex poisoned".to_string())?.get(&id)?;
        let src = PathBuf::from(&rec.path);
        // Keep successful exports on disk after exit: CF_HDROP references the
        // file itself, and Discord/Explorer may open it well after this command.
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        let dir = Config::dir().join("shares").join(format!("{}_{nonce}", rec.id));
        let dst = dir.join(src.file_name().ok_or("invalid clip path")?);
        let muted = muted_tracks.unwrap_or_default();
        let volumes = track_volumes.unwrap_or_default();
        let result = export_share_clip(
            &ffmpeg_path(), &src, &dst,
            &TrimOpts {
                start_secs: start_secs.unwrap_or(0.0),
                end_secs: end_secs.unwrap_or(rec.duration_ms as f64 / 1000.0),
                audio_tracks: rec.audio_tracks,
                muted_tracks: &muted,
                track_volumes: &volumes,
            },
        ).and_then(|path| copy_file_to_clipboard(&path));
        if result.is_err() {
            let _ = std::fs::remove_file(&dst);
            let _ = std::fs::remove_dir(&dir);
        }
        result
    }).await
}

#[tauri::command]
fn reveal_in_explorer(path: String) -> Result<(), String> {
    encore_common::platform::reveal_path(Path::new(&path))
}

#[tauri::command]
fn get_platform_capabilities() -> encore_common::platform::PlatformCapabilities {
    encore_common::platform::capabilities()
}

#[cfg(windows)]
const APP_ICO: &[u8] = include_bytes!("../icons/icon.ico");

/// Select the PNG/DIB slice inside an `.ico` payload whose dimension best matches `target_px`.
#[cfg(windows)]
fn pick_ico_entry(ico: &[u8], target_px: u32) -> Option<&[u8]> {
    if ico.len() < 6 || u16::from_le_bytes([ico[0], ico[1]]) != 0 || u16::from_le_bytes([ico[2], ico[3]]) != 1 {
        return None;
    }
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let mut best: Option<(u32, u32, &[u8])> = None;
    for i in 0..count {
        let off = 6 + i * 16;
        if off + 16 > ico.len() {
            break;
        }
        let w = if ico[off] == 0 { 256 } else { ico[off] as u32 };
        let size = u32::from_le_bytes([ico[off + 8], ico[off + 9], ico[off + 10], ico[off + 11]]) as usize;
        let data_off = u32::from_le_bytes([ico[off + 12], ico[off + 13], ico[off + 14], ico[off + 15]]) as usize;
        if data_off.saturating_add(size) > ico.len() {
            continue;
        }
        let slice = &ico[data_off..data_off + size];
        let diff = w.abs_diff(target_px);
        let prefer_upscale_penalty = u32::from(w < target_px);
        match best {
            Some((bd, bp, _)) if (prefer_upscale_penalty, diff) >= (bp, bd) => {}
            _ => best = Some((diff, prefer_upscale_penalty, slice)),
        }
    }
    best.map(|(_, _, s)| s)
}

#[cfg(windows)]
fn apply_windows_taskbar_icon(app: &AppHandle) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, GetSystemMetrics, SendMessageW, ICON_BIG, ICON_SMALL,
        LR_DEFAULTCOLOR, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, WM_SETICON,
    };

    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let Ok(raw_hwnd) = win.hwnd() else {
        return;
    };
    let hwnd = HWND(raw_hwnd.0);
    unsafe {
        let big_w = GetSystemMetrics(SM_CXICON).max(32);
        let big_h = GetSystemMetrics(SM_CYICON).max(32);
        if let Some(bytes) = pick_ico_entry(APP_ICO, big_w as u32) {
            if let Ok(hicon) = CreateIconFromResourceEx(bytes, true, 0x0003_0000, big_w, big_h, LR_DEFAULTCOLOR) {
                let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(hicon.0 as isize)));
            }
        }
        let sm_w = GetSystemMetrics(SM_CXSMICON).max(16);
        let sm_h = GetSystemMetrics(SM_CYSMICON).max(16);
        if let Some(bytes) = pick_ico_entry(APP_ICO, sm_w as u32) {
            if let Ok(hicon) = CreateIconFromResourceEx(bytes, true, 0x0003_0000, sm_w, sm_h, LR_DEFAULTCOLOR) {
                let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(hicon.0 as isize)));
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let Some(_instance_guard) = tray::acquire_single_instance() else {
        return;
    };
    let state = AppState::open().expect("failed to open Encore clip database");
    let builder = tauri::Builder::default();
    #[cfg(windows)]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    builder
        .manage(updater::UpdateState::default())
        .manage(state)
        .setup(move |app| {
            if let Ok(resources) = app.path().resource_dir() {
                encore_common::platform::set_resource_dir(resources);
            }
            #[cfg(windows)]
            apply_windows_taskbar_icon(app.handle());
            tray::start_wake_listener(app.handle().clone());
            // Restore an older recorder's hidden tray as well. This is not needed for
            // newly launched recorders, which always start with their own visible icon.
            let handle = app.handle().clone();
            let startup = lifecycle::global().begin_task()?;
            tauri::async_runtime::spawn_blocking(move || {
                let _startup = startup;
                updater::resume_after_update(&handle);
                let _ = ipc::send_request(&IpcRequest::SetTrayVisible(true));
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    tray::close_library(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            updater::check_for_updates,
            updater::install_update,
            get_recorder_status,
            start_recorder,
            stop_recorder,
            trigger_save_clip,
            list_audio_devices,
            list_audio_applications,
            list_capture_applications,
            pick_game_executable,
            get_platform_capabilities,
            get_config,
            save_config,
            pick_clips_folder,
            sync_and_list_clips,
            toggle_favorite,
            update_clip_details,
            delete_clip,
            get_clip_waveforms,
            get_clip_audio_tracks,
            trim_clip_cmd,
            export_vertical_cmd,
            build_montage_cmd,
            copy_clip_to_clipboard,
            reveal_in_explorer,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Encore application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ffmpeg() -> PathBuf { encore_common::ffmpeg_path() }

    #[test]
    fn unique_output_path_avoids_collisions() {
        let dir = std::env::temp_dir().join(format!("encore_ui_path_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let p1 = unique_output_path(&dir, "Valorant", "_trim");
        assert_eq!(p1.file_name().unwrap(), "Valorant_trim.mp4");
        std::fs::write(&p1, b"x").unwrap();

        let p2 = unique_output_path(&dir, "Valorant", "_trim");
        assert_eq!(p2.file_name().unwrap(), "Valorant_trim_2.mp4");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_three_m4a_tracks_for_editor_preview() {
        let ff = test_ffmpeg();
        let dir = std::env::temp_dir().join(format!("encore_ui_tracks_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = dir.join("multi.mp4");
        let st = Command::new(&ff)
            .args([
                "-y", "-v", "error",
                "-f", "lavfi", "-i", "color=c=black:s=320x240:r=30:d=1",
                "-f", "lavfi", "-i", "sine=frequency=440:duration=1",
                "-f", "lavfi", "-i", "sine=frequency=660:duration=1",
                "-f", "lavfi", "-i", "sine=frequency=880:duration=1",
                "-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:a",
                "-c:v", "libx264", "-c:a", "aac",
            ])
            .arg(&src)
            .status()
            .unwrap();
        assert!(st.success());

        let cache = dir.join("cache");
        let files = extract_track_audio_files(&ff, "clip_abc", &src, 3, &cache).unwrap();
        assert_eq!(files.len(), 3);
        for f in files {
            assert!(Path::new(&f).exists());
            assert!(std::fs::metadata(&f).unwrap().len() > 200);
        }

        // Audio endpoint availability belongs to capture integration tests, not media extraction.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(windows)]
    fn app_ico_builds_taskbar_and_small_hicons() {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateIconFromResourceEx, DestroyIcon, LR_DEFAULTCOLOR,
        };
        for px in [16u32, 32, 48, 64] {
            let bytes = pick_ico_entry(APP_ICO, px).expect("ico entry");
            let icon = unsafe {
                CreateIconFromResourceEx(bytes, true, 0x0003_0000, px as i32, px as i32, LR_DEFAULTCOLOR)
            }
            .expect("hicon");
            unsafe {
                let _ = DestroyIcon(icon);
            }
        }
    }
}
