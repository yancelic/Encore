#![cfg_attr(all(not(debug_assertions), not(test)), windows_subsystem = "windows")]
//! Encore headless recorder: multi-track WGC/MF capture, tray icon, named-pipe IPC, global save hotkey.
mod audio;
mod clip;
mod clock;
mod encoder;
mod ipc;
mod procs;
mod recorder;
mod target;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use encore_common::ipc::{RecorderState, RecorderStatus};
use encore_common::tray_icon::TrayBadge;
use encore_common::{parse_hotkey, CaptureMode, Config};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
    PostQuitMessage, RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG, WM_APP, WM_COMMAND,
    WM_DESTROY, WNDCLASSW,
};
use windows::core::w;

const WM_UPDATE_TRAY: u32 = WM_APP + 20;
const WM_TRAY_CMD: u32 = WM_APP + 21;
const WM_TRAY_EVENT: u32 = WM_APP + 22;

fn apply_cli_overrides(cfg: &mut Config, args: &[String]) {
    if let Some(d) = args.iter().skip_while(|a| *a != "--device").nth(1) {
        cfg.audio_device = Some(d.clone());
    }
    if let Some(m) = args.iter().skip_while(|a| *a != "--mode").nth(1) {
        cfg.mode = if m.starts_with('w') { CaptureMode::ActiveWindow } else { CaptureMode::GameDetection };
    }
    if let Some(f) = args.iter().skip_while(|a| *a != "--fps").nth(1).and_then(|s| s.parse::<u32>().ok()) {
        cfg.fps = f.clamp(1, 240);
    }
    if let Some(h) = args.iter().skip_while(|a| *a != "--height").nth(1).and_then(|s| s.parse::<u32>().ok()) {
        cfg.height = h;
    }
}

fn badge_for_status(st: &RecorderStatus) -> TrayBadge {
    match st.state {
        RecorderState::Idle => TrayBadge::Ready,
        RecorderState::Recording | RecorderState::Saving => TrayBadge::Recording,
    }
}

fn format_status_text(st: &RecorderStatus) -> (String, String) {
    let tip = match st.state {
        RecorderState::Idle => format!("Encore - Ready ({:?})", st.mode),
        RecorderState::Recording => format!("Encore - Recording {}", st.target.as_deref().unwrap_or("")),
        RecorderState::Saving => "Encore - Saving clip...".into(),
    };
    let menu = match st.state {
        RecorderState::Idle => "Status: Ready to capture".into(),
        RecorderState::Recording => format!("Status: Recording {}", st.target.as_deref().unwrap_or("")),
        RecorderState::Saving => "Status: Saving clip...".into(),
    };
    (tip, menu)
}

unsafe extern "system" fn msg_wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        tray::WM_TRAY_CALLBACK => {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_TRAY_EVENT, wp, lp);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_TRAY_CMD, wp, lp);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

fn try_register_hotkey(manager: &GlobalHotKeyManager, hotkey_str: &str) -> Result<HotKey, String> {
    let hk = parse_hotkey(hotkey_str)?;
    manager.register(hk).map_err(|e| {
        format!(
            "Could not register hotkey {hotkey_str}: {e}. Another app probably uses it; change the hotkey in Settings."
        )
    })?;
    Ok(hk)
}

fn create_message_window() -> HWND {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(msg_wnd_proc),
            lpszClassName: w!("EncoreRecorderMsgWin"),
            ..Default::default()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            Default::default(),
            w!("EncoreRecorderMsgWin"),
            w!("EncoreRecorder"),
            Default::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
        .expect("create message window")
    }
}

fn main() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--audio-diag") {
        audio::diag();
        return;
    }

    // Single-instance lock so two recorders don't fight over the hotkey and IPC pipe.
    let _singleton = unsafe {
        let h = CreateMutexW(None, false, w!("Local\\EncoreRecorderSingleton")).ok();
        if GetLastError() == ERROR_ALREADY_EXISTS {
            eprintln!("Encore recorder is already running.");
            return;
        }
        h
    };

    let mut initial_cfg = Config::load();
    apply_cli_overrides(&mut initial_cfg, &args);
    let res_label = if initial_cfg.height == 0 { "native".to_string() } else { format!("{}p", initial_cfg.height) };
    println!(
        "Encore recorder - mode {:?}, hotkey {}, clip length {}s, {}, {} fps",
        initial_cfg.mode, initial_cfg.hotkey, initial_cfg.buffer_secs, res_label, initial_cfg.fps
    );

    let manager = GlobalHotKeyManager::new().expect("hotkey manager");
    let (mut current_hotkey, initial_hotkey_err) = match try_register_hotkey(&manager, &initial_cfg.hotkey) {
        Ok(hk) => {
            println!("Press {} to save the last {}s.", initial_cfg.hotkey, initial_cfg.buffer_secs);
            (Some(hk), None)
        }
        Err(e) => {
            eprintln!("{e}");
            (None, Some(e))
        }
    };

    let hwnd = create_message_window();
    let hotkey_error = Arc::new(Mutex::new(initial_hotkey_err.clone()));
    let status = Arc::new(Mutex::new(RecorderStatus {
        state: RecorderState::Idle,
        target: None,
        mode: initial_cfg.mode,
        fps: initial_cfg.fps,
        height: initial_cfg.height,
        buffer_secs: initial_cfg.buffer_secs,
        hotkey: initial_cfg.hotkey.clone(),
        last_clip: None,
        error: initial_hotkey_err,
    }));
    let cfg_shared = Arc::new(Mutex::new(initial_cfg.clone()));
    let save = Arc::new(AtomicBool::new(false));
    let reload = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));

    let no_tray = args.iter().any(|a| a == "--no-tray");
    let mut tray_icon = {
        let st = status.lock().unwrap();
        tray::TrayIcon::new(hwnd, &format_status_text(&st).0, badge_for_status(&st), !no_tray)
    };

    let hwnd_raw = hwnd.0 as isize;
    let on_status_change = Arc::new(move || unsafe {
        let _ = PostMessageW(Some(HWND(hwnd_raw as *mut _)), WM_UPDATE_TRAY, WPARAM(0), LPARAM(0));
    });
    target::supervise(
        cfg_shared.clone(),
        save.clone(),
        reload.clone(),
        stop.clone(),
        status.clone(),
        hotkey_error.clone(),
        on_status_change,
    );

    let ipc_shared = Arc::new(ipc::IpcShared {
        status: status.clone(),
        reload_ack: Mutex::new(None),
    });
    ipc::start(hwnd, ipc_shared.clone(), stop.clone());

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);

            match msg.message {
                WM_TRAY_EVENT | tray::WM_TRAY_CALLBACK => {
                    let (menu_status, hk) = {
                        let st = status.lock().unwrap();
                        (format_status_text(&st).1, st.hotkey.clone())
                    };
                    if let Some(cmd) = tray_icon.handle_callback(msg.lParam, &menu_status, &hk) {
                        let _ = PostMessageW(Some(hwnd), WM_TRAY_CMD, WPARAM(cmd), LPARAM(0));
                    }
                }
                WM_TRAY_CMD => {
                    let cmd = msg.wParam.0 & 0xffff;
                    match cmd {
                        tray::CMD_SAVE_CLIP => {
                            println!("Saving clip (tray)...");
                            save.store(true, Ordering::Relaxed);
                        }
                        tray::CMD_OPEN_UI => {
                            let dir = cfg_shared.lock().unwrap().clips_dir.clone();
                            tray::launch_ui_or_folder(&dir);
                        }
                        tray::CMD_OPEN_FOLDER => {
                            let dir = cfg_shared.lock().unwrap().clips_dir.clone();
                            tray::open_folder(&dir);
                        }
                        tray::CMD_QUIT => {
                            stop.store(true, Ordering::Relaxed);
                            let _ = DestroyWindow(hwnd);
                        }
                        _ => {}
                    }
                }
                WM_UPDATE_TRAY => {
                    let (tip, badge) = {
                        let st = status.lock().unwrap();
                        (format_status_text(&st).0, badge_for_status(&st))
                    };
                    tray_icon.set_state(&tip, badge);
                }
                ipc::WM_IPC_SET_TRAY => {
                    let (tip, badge) = {
                        let st = status.lock().unwrap();
                        (format_status_text(&st).0, badge_for_status(&st))
                    };
                    tray_icon.set_visible(msg.wParam.0 != 0, &tip, badge);
                }
                ipc::WM_IPC_SAVE => {
                    println!("Saving clip (IPC)...");
                    save.store(true, Ordering::Relaxed);
                }
                ipc::WM_IPC_RELOAD => {
                    let mut new_cfg = Config::load();
                    apply_cli_overrides(&mut new_cfg, &args);
                    let res = match parse_hotkey(&new_cfg.hotkey) {
                        Ok(new_hk) if current_hotkey.is_some_and(|cur| cur.id() == new_hk.id()) => Ok(()),
                        Ok(_) => {
                            if let Some(prev) = current_hotkey {
                                let _ = manager.unregister(prev);
                            }
                            match try_register_hotkey(&manager, &new_cfg.hotkey) {
                                Ok(new_hk) => {
                                    current_hotkey = Some(new_hk);
                                    *hotkey_error.lock().unwrap() = None;
                                    Ok(())
                                }
                                Err(e) => {
                                    if let Some(prev) = current_hotkey {
                                        if manager.register(prev).is_err() {
                                            current_hotkey = None;
                                            *hotkey_error.lock().unwrap() = Some(e.clone());
                                        }
                                    } else {
                                        *hotkey_error.lock().unwrap() = Some(e.clone());
                                    }
                                    Err(e)
                                }
                            }
                        }
                        Err(e) => Err(e),
                    };
                    if res.is_ok() {
                        *cfg_shared.lock().unwrap() = new_cfg.clone();
                        {
                            let mut st = status.lock().unwrap();
                            st.mode = new_cfg.mode;
                            st.fps = new_cfg.fps;
                            st.height = new_cfg.height;
                            st.buffer_secs = new_cfg.buffer_secs;
                            st.hotkey = new_cfg.hotkey;
                            st.error = None;
                        }
                        reload.store(true, Ordering::Relaxed);
                        let (tip, badge) = {
                            let st = status.lock().unwrap();
                            (format_status_text(&st).0, badge_for_status(&st))
                        };
                        tray_icon.set_state(&tip, badge);
                    }
                    if let Some(ack) = ipc_shared.reload_ack.lock().unwrap().take() {
                        let _ = ack.send(res);
                    }
                }
                ipc::WM_IPC_QUIT => {
                    stop.store(true, Ordering::Relaxed);
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }

            while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
                if current_hotkey.is_some_and(|hk| ev.id == hk.id()) && ev.state == HotKeyState::Pressed {
                    println!("Saving clip...");
                    save.store(true, Ordering::Relaxed);
                }
            }
        }
    }
    stop.store(true, Ordering::Relaxed);
}
