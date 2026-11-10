//! Decides *what* to record and keeps a capture session running for it.
//!
//! - `GameDetection`: records the monitor a known game's window is on; idle (zero cost) otherwise.
//! - `ActiveWindow`: records whichever window has focus.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use encore_common::ipc::{RecorderState, RecorderStatus};
use encore_common::{pretty_game_name, CaptureMode, Config};
use windows_capture::window::Window;

use crate::encoder::Err;
use crate::procs;
use crate::recorder::{self, CaptureControl, CaptureTarget};

/// What we are currently capturing. Raw handles keep this `Copy + Eq` and `Send`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Monitor(usize),
    Window(usize),
}

/// Pure decision: does `exe` match one of the configured game names?
pub fn is_game(exe: &str, games: &[String]) -> bool {
    games.iter().any(|g| g.eq_ignore_ascii_case(exe))
}

fn find_game_monitor(games: &[String]) -> Option<(Target, String)> {
    let procs = procs::snapshot();
    for w in Window::enumerate().ok()? {
        let Some(name) = w.process_id().ok().and_then(|pid| procs.get(&pid).cloned()) else { continue };
        if !is_game(&name, games) {
            continue;
        }
        if let Some(m) = w.monitor() {
            return Some((Target::Monitor(m.as_raw_hmonitor() as usize), name));
        }
    }
    None
}

fn find_active_window() -> Option<(Target, String)> {
    let w = Window::foreground().ok()?;
    if !w.is_valid() {
        return None;
    }
    let name = procs::snapshot().get(&w.process_id().ok()?)?.clone();
    let skip = [
        "encore-recorder.exe",
        "encore-ui.exe",
        "explorer.exe",
        "searchhost.exe",
        "shellexperiencehost.exe",
    ];
    if skip.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
        return None;
    }
    if w.width().unwrap_or(0) < 200 || w.height().unwrap_or(0) < 200 {
        return None;
    }
    Some((Target::Window(w.as_raw_hwnd() as usize), name))
}

fn desired(cfg: &Config) -> Option<(Target, String)> {
    match cfg.mode {
        CaptureMode::GameDetection => find_game_monitor(&cfg.games),
        CaptureMode::ActiveWindow => find_active_window(),
    }
}

fn start_session(
    t: Target,
    exe_name: &str,
    cfg: &Config,
    save: Arc<AtomicBool>,
    status: Arc<Mutex<RecorderStatus>>,
) -> Result<CaptureControl, Err> {
    let game_name = Some(pretty_game_name(exe_name));
    match t {
        Target::Monitor(h) => recorder::start(CaptureTarget::Monitor(h), game_name, cfg, save, status),
        Target::Window(h) => recorder::start(CaptureTarget::Window(h), game_name, cfg, save, status),
    }
}

/// Polls every 2 s, reacting immediately to `reload` / `stop`, and keeps tray/IPC status updated.
pub fn supervise(
    cfg: Arc<Mutex<Config>>,
    save: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<RecorderStatus>>,
    hotkey_error: Arc<Mutex<Option<String>>>,
    on_status_change: Arc<dyn Fn() + Send + Sync>,
) {
    thread::spawn(move || {
        let mut current: Option<(Target, CaptureControl)> = None;
        let mut announced_wait = false;
        while !stop.load(Ordering::Relaxed) {
            let cfg_snap = cfg.lock().unwrap().clone();
            if reload.swap(false, Ordering::Relaxed) {
                if let Some((_, ctl)) = current.take() {
                    let _ = ctl.stop();
                    println!("Capture restarted for updated settings.");
                }
            }
            let want = desired(&cfg_snap);
            let same = match (&current, &want) {
                (Some((c, _)), Some((w, _))) => c == w,
                (None, None) => true,
                _ => false,
            };
            if !same {
                if let Some((_, ctl)) = current.take() {
                    let _ = ctl.stop();
                    println!("Capture stopped.");
                }
                if let Some((t, name)) = &want {
                    println!("Recording {name} ({t:?})");
                    {
                        let mut st = status.lock().unwrap();
                        st.state = RecorderState::Recording;
                        st.target = Some(name.clone());
                        st.error = hotkey_error.lock().unwrap().clone();
                    }
                    on_status_change();
                    match start_session(*t, name, &cfg_snap, save.clone(), status.clone()) {
                        Ok(ctl) => current = Some((*t, ctl)),
                        Err(e) => {
                            eprintln!("could not start capture: {e}");
                            let mut st = status.lock().unwrap();
                            st.state = RecorderState::Idle;
                            st.target = None;
                            st.error = Some(e.to_string());
                            on_status_change();
                        }
                    }
                } else {
                    {
                        let mut st = status.lock().unwrap();
                        st.state = RecorderState::Idle;
                        st.target = None;
                        st.error = hotkey_error.lock().unwrap().clone();
                    }
                    on_status_change();
                }
            }
            if want.is_some() {
                announced_wait = false;
            } else if !announced_wait && cfg_snap.mode == CaptureMode::GameDetection {
                println!("Waiting for a game to start...");
                announced_wait = true;
            }
            for _ in 0..20 {
                if stop.load(Ordering::Relaxed) || reload.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        if let Some((_, ctl)) = current.take() {
            let _ = ctl.stop();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_names_match_case_insensitively() {
        let g = vec!["VALORANT-Win64-Shipping.exe".to_string()];
        assert!(is_game("valorant-win64-shipping.exe", &g));
        assert!(!is_game("chrome.exe", &g));
    }
}
