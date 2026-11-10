//! System tray icon (with live Ready/REC status badge), close-to-tray behavior, and single-instance wake.
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use encore_common::ipc::{self, IpcRequest, IpcResponse, RecorderState, RecorderStatus, UI_WAKE_PIPE};
use encore_common::tray_icon::{render_tray_rgba, TrayBadge, ICON_DIM};
use encore_common::Config;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub const TRAY_ID: &str = "encore-tray";

pub struct TrayController {
    pub quitting: Arc<AtomicBool>,
    sync_signal: Arc<(Mutex<()>, Condvar)>,
}

impl TrayController {
    pub fn new() -> Self {
        Self {
            quitting: Arc::new(AtomicBool::new(false)),
            sync_signal: Arc::new((Mutex::new(()), Condvar::new())),
        }
    }

    pub fn notify_sync(&self) {
        self.sync_signal.1.notify_all();
    }
}

#[cfg(windows)]
pub struct SingleInstanceGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Ensure only one `encore-ui` process runs; if already running, wake its window and return `None`.
#[cfg(windows)]
pub fn acquire_single_instance() -> Option<SingleInstanceGuard> {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::w;

    unsafe {
        let Ok(h) = CreateMutexW(None, false, w!("Local\\EncoreUiSingleton")) else {
            return None;
        };
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = windows::Win32::Foundation::CloseHandle(h);
            for _ in 0..5 {
                if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(UI_WAKE_PIPE) {
                    let _ = f.write_all(b"show\n");
                    let _ = f.flush();
                    break;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            return None;
        }
        Some(SingleInstanceGuard(h))
    }
}

#[cfg(not(windows))]
pub fn acquire_single_instance() -> Option<()> {
    Some(())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[cfg(windows)]
pub fn start_wake_listener(app: AppHandle, quitting: Arc<AtomicBool>) {
    use std::os::windows::io::FromRawHandle;
    use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows::core::HSTRING;

    std::thread::spawn(move || {
        let hname = HSTRING::from(UI_WAKE_PIPE);
        while !quitting.load(Ordering::Relaxed) {
            let handle = unsafe {
                CreateNamedPipeW(
                    &hname,
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    2,
                    256,
                    256,
                    0,
                    None,
                )
            };
            if handle == INVALID_HANDLE_VALUE || handle.is_invalid() {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            if quitting.load(Ordering::Relaxed) {
                unsafe { let _ = CloseHandle(handle); }
                break;
            }
            let connected = unsafe {
                match ConnectNamedPipe(handle, None) {
                    Ok(()) => true,
                    Err(e) => e.code().0 as u32 == ERROR_PIPE_CONNECTED.0,
                }
            };
            if !connected || quitting.load(Ordering::Relaxed) {
                unsafe {
                    let _ = DisconnectNamedPipe(handle);
                    let _ = CloseHandle(handle);
                }
                continue;
            }
            let file = unsafe { std::fs::File::from_raw_handle(handle.0) };
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            if reader.read_line(&mut line).is_ok() && line.trim() == "show" {
                show_main_window(&app);
            }
            unsafe { let _ = DisconnectNamedPipe(handle); }
        }
    });
}

#[cfg(windows)]
fn wake_pipe_to_stop() {
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .open(UI_WAKE_PIPE)
        .and_then(|mut f| f.write_all(b"stop\n"));
}

#[cfg(not(windows))]
fn wake_pipe_to_stop() {}

pub fn tray_visual_state(status: Option<&RecorderStatus>, fallback_hotkey: &str) -> (TrayBadge, String, String, String) {
    match status {
        None => (
            TrayBadge::Offline,
            "Encore - Recorder Offline".into(),
            "Status: Recorder Offline".into(),
            fallback_hotkey.to_string(),
        ),
        Some(st) => {
            let badge = match st.state {
                RecorderState::Idle => TrayBadge::Ready,
                RecorderState::Recording | RecorderState::Saving => TrayBadge::Recording,
            };
            let tip = match st.state {
                RecorderState::Idle => format!("Encore - Ready ({:?})", st.mode),
                RecorderState::Recording => format!("Encore - Recording {}", st.target.as_deref().unwrap_or("")),
                RecorderState::Saving => "Encore - Saving clip...".into(),
            };
            let menu_status = match st.state {
                RecorderState::Idle => "Status: Ready to capture".into(),
                RecorderState::Recording => format!("Status: Recording {}", st.target.as_deref().unwrap_or("")),
                RecorderState::Saving => "Status: Saving clip...".into(),
            };
            (badge, tip, menu_status, st.hotkey.clone())
        }
    }
}

struct TrayMenuItems {
    status: MenuItem<tauri::Wry>,
    save: MenuItem<tauri::Wry>,
    toggle: MenuItem<tauri::Wry>,
}

fn build_menu(app: &AppHandle, status_line: &str, online: bool, hotkey: &str) -> tauri::Result<(Menu<tauri::Wry>, TrayMenuItems)> {
    let status_item = MenuItem::with_id(app, "status", status_line, false, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let open_item = MenuItem::with_id(app, "open", "Open Encore", true, None::<&str>)?;
    let save_item = MenuItem::with_id(app, "save", format!("Save Clip ({hotkey})"), online, None::<&str>)?;
    let toggle_label = if online { "Stop Recorder" } else { "Start Recorder" };
    let toggle_item = MenuItem::with_id(app, "toggle_rec", toggle_label, true, None::<&str>)?;
    let folder_item = MenuItem::with_id(app, "folder", "Open Clips Folder", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Encore", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status_item,
            &sep1,
            &open_item,
            &save_item,
            &toggle_item,
            &folder_item,
            &sep2,
            &quit_item,
        ],
    )?;
    Ok((
        menu,
        TrayMenuItems {
            status: status_item,
            save: save_item,
            toggle: toggle_item,
        },
    ))
}

fn query_recorder_status() -> Option<RecorderStatus> {
    match ipc::send_request(&IpcRequest::GetStatus) {
        Ok(IpcResponse::Status(s)) => Some(s),
        _ => None,
    }
}

pub fn setup_tray(app: &AppHandle, ctrl: Arc<TrayController>) -> tauri::Result<()> {
    let initial_status = query_recorder_status();
    if initial_status.is_some() {
        let _ = ipc::send_request(&IpcRequest::SetTrayVisible(false));
    }
    let cfg = Config::load();
    let (badge, tip, status_line, hotkey) = tray_visual_state(initial_status.as_ref(), &cfg.hotkey);
    let (menu, items) = build_menu(app, &status_line, initial_status.is_some(), &hotkey)?;
    let icon = Image::new_owned(render_tray_rgba(badge), ICON_DIM as u32, ICON_DIM as u32);

    let ctrl_menu = ctrl.clone();
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip(&tip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => show_main_window(tray.app_handle()),
            _ => {}
        })
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            match id {
                "open" => show_main_window(app),
                "save" => {
                    std::thread::spawn(|| {
                        let _ = ipc::send_request(&IpcRequest::SaveClip);
                    });
                }
                "toggle_rec" => {
                    let res_dir: Option<PathBuf> = app.path().resource_dir().ok();
                    let signal = ctrl_menu.clone();
                    std::thread::spawn(move || {
                        if query_recorder_status().is_some() {
                            let _ = ipc::send_request(&IpcRequest::Shutdown);
                        } else {
                            let _ = crate::launch_recorder_blocking(res_dir.as_deref());
                        }
                        signal.notify_sync();
                    });
                }
                "folder" => {
                    let dir = Config::load().clips_dir;
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = Command::new("explorer.exe").arg(dir).spawn();
                }
                "quit" => {
                    ctrl_menu.quitting.store(true, Ordering::Relaxed);
                    ctrl_menu.notify_sync();
                    wake_pipe_to_stop();
                    let _ = ipc::send_request(&IpcRequest::Shutdown);
                    let _ = app.remove_tray_by_id(TRAY_ID);
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;

    #[cfg(windows)]
    start_wake_listener(app.clone(), ctrl.quitting.clone());

    let app_sync = app.clone();
    std::thread::spawn(move || {
        let mut prev_key = (badge, status_line, initial_status.is_some(), hotkey);
        let mut was_online = initial_status.is_some();
        while !ctrl.quitting.load(Ordering::Relaxed) {
            {
                let (lock, cvar) = &*ctrl.sync_signal;
                let guard = lock.lock().unwrap();
                let _ = cvar.wait_timeout(guard, Duration::from_millis(1200));
            }
            if ctrl.quitting.load(Ordering::Relaxed) {
                break;
            }
            let st = query_recorder_status();
            let online = st.is_some();
            if online && !was_online {
                let _ = ipc::send_request(&IpcRequest::SetTrayVisible(false));
            }
            was_online = online;

            let cfg_hk = Config::load().hotkey;
            let (new_badge, new_tip, new_status_line, new_hotkey) = tray_visual_state(st.as_ref(), &cfg_hk);
            let next_key = (new_badge, new_status_line.clone(), online, new_hotkey.clone());
            if next_key != prev_key {
                let icon_changed = next_key.0 != prev_key.0;
                let app_ui = app_sync.clone();
                let st_item = items.status.clone();
                let sv_item = items.save.clone();
                let tg_item = items.toggle.clone();
                let _ = app_sync.run_on_main_thread(move || {
                    if let Some(tray) = app_ui.tray_by_id(TRAY_ID) {
                        if icon_changed {
                            let img = Image::new_owned(render_tray_rgba(new_badge), ICON_DIM as u32, ICON_DIM as u32);
                            let _ = tray.set_icon(Some(img));
                        }
                        let _ = tray.set_tooltip(Some(&new_tip));
                    }
                    let _ = st_item.set_text(&new_status_line);
                    let _ = sv_item.set_text(format!("Save Clip ({new_hotkey})"));
                    let _ = sv_item.set_enabled(online);
                    let _ = tg_item.set_text(if online { "Stop Recorder" } else { "Start Recorder" });
                });
                prev_key = next_key;
            }
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use encore_common::CaptureMode;

    #[test]
    fn tray_visual_state_maps_offline_ready_and_recording() {
        let (b0, _, s0, h0) = tray_visual_state(None, "Alt+S");
        assert_eq!(b0, TrayBadge::Offline);
        assert!(s0.contains("Offline"));
        assert_eq!(h0, "Alt+S");

        let mut st = RecorderStatus {
            state: RecorderState::Idle,
            target: None,
            mode: CaptureMode::GameDetection,
            fps: 60,
            height: 1080,
            buffer_secs: 60,
            hotkey: "Alt+Backquote".into(),
            last_clip: None,
            error: None,
        };
        assert_eq!(tray_visual_state(Some(&st), "Alt+S").0, TrayBadge::Ready);
        st.state = RecorderState::Recording;
        st.target = Some("VALORANT".into());
        assert_eq!(tray_visual_state(Some(&st), "Alt+S").0, TrayBadge::Recording);
        st.state = RecorderState::Saving;
        assert_eq!(tray_visual_state(Some(&st), "Alt+S").0, TrayBadge::Recording);
    }
}
