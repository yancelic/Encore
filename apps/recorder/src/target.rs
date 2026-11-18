//! Decides *what* to record and keeps a capture session running for it.
//!
//! - `GameDetection`: stays attached to a known game's main window across focus changes.
//! - `ScreenCapture`: records the primary display independently of focus.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use encore_common::ipc::{RecorderState, RecorderStatus};
use encore_common::{pretty_game_name, CaptureMode, Config};
use windows_capture::window::Window;
use windows_capture::monitor::Monitor;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindow, IsIconic, GW_OWNER};

use crate::encoder::Err;
use crate::procs;
use crate::recorder::{self, CaptureControl, CaptureTarget};

/// Include PID so a handle reused by another process changes the capture target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Target {
    hwnd: usize,
    pid: u32,
}

#[derive(Debug, Clone)]
struct GameWindow {
    target: Target,
    name: String,
    area: u64,
    owned: bool,
    minimized: bool,
}

struct CaptureSelection {
    target: CaptureTarget,
    name: String,
    game_window: Option<Target>,
    minimized: bool,
}

/// Pure decision: does `exe` match one of the configured game names?
pub fn is_game(exe: &str, games: &[String]) -> bool {
    games.iter().any(|g| g.eq_ignore_ascii_case(exe))
}

/// Stay with the selected game. Prefer a main window over dialogs/splash screens.
fn select_game_window(
    candidates: &[GameWindow],
    preferred: Option<Target>,
    foreground: Option<usize>,
) -> Option<&GameWindow> {
    if let Some(previous) = preferred {
        if let Some(w) = candidates.iter().find(|w| w.target == previous && w.minimized) {
            return Some(w);
        }
        if let Some(w) = candidates.iter().filter(|w| w.target.pid == previous.pid).max_by_key(|w| {
            (!w.owned, w.area, w.target == previous, std::cmp::Reverse(w.target.hwnd))
        }) {
            return Some(w);
        }
    }
    let foreground_pid = candidates.iter().find(|w| Some(w.target.hwnd) == foreground).map(|w| w.target.pid);
    candidates.iter().max_by_key(|w| {
        (Some(w.target.pid) == foreground_pid, !w.minimized, !w.owned, w.area, std::cmp::Reverse(w.target.hwnd))
    })
}

fn find_game_window(games: &[String], preferred: Option<Target>) -> Option<GameWindow> {
    let processes = procs::snapshot();
    let mut candidates = Vec::new();
    for w in Window::enumerate().ok()? {
        let Ok(pid) = w.process_id() else { continue };
        let Some(name) = processes.get(&pid) else { continue };
        if !is_game(name, games) {
            continue;
        }
        let target = Target { hwnd: w.as_raw_hwnd() as usize, pid };
        let hwnd = HWND(w.as_raw_hwnd());
        let minimized = unsafe { IsIconic(hwnd).as_bool() };
        let mut rect = RECT::default();
        if unsafe { GetClientRect(hwnd, &mut rect) }.is_err() {
            continue;
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if (width < 200 || height < 200) && !(minimized && preferred == Some(target)) {
            continue;
        }
        candidates.push(GameWindow {
            target, name: name.clone(),
            area: width.max(0) as u64 * height.max(0) as u64,
            owned: unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok(), minimized,
        });
    }
    let foreground = Window::foreground().ok().map(|w| w.as_raw_hwnd() as usize);
    select_game_window(&candidates, preferred, foreground).cloned()
}

fn desired(cfg: &Config, preferred: Option<Target>) -> Option<CaptureSelection> {
    match cfg.mode {
        CaptureMode::GameDetection => find_game_window(&cfg.games, preferred).map(|window| CaptureSelection {
            target: CaptureTarget::Window(window.target.hwnd),
            name: window.name,
            game_window: Some(window.target),
            minimized: window.minimized,
        }),
        CaptureMode::ScreenCapture => Monitor::primary().ok().map(|monitor| CaptureSelection {
            target: CaptureTarget::Monitor(monitor.as_raw_hmonitor() as usize),
            name: "Primary display".into(),
            game_window: None,
            minimized: false,
        }),
    }
}

fn start_session(
    selected: &CaptureSelection,
    cfg: &Config,
    save: Arc<AtomicBool>,
    status: Arc<Mutex<RecorderStatus>>,
) -> Result<CaptureControl, Err> {
    let game_name = selected.game_window.map(|_| pretty_game_name(&selected.name));
    recorder::start(selected.target, game_name, cfg, save, status)
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
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut current: Option<(CaptureTarget, Option<Target>, CaptureControl)> = None;
        let mut preferred = None;
        let mut announced_wait = false;
        while !stop.load(Ordering::Relaxed) {
            let cfg_snap = cfg.lock().unwrap().clone();
            let reloaded = reload.swap(false, Ordering::Relaxed);
            if reloaded {
                preferred = None;
            }
            let selected = desired(&cfg_snap, preferred);
            preferred = selected.as_ref().and_then(|w| w.game_window);
            // Keep the minimized game selected, but stop capture until restoration.
            // The new session starts a fresh replay history.
            let want = selected.filter(|w| !w.minimized);
            let same = !reloaded && match (&current, &want) {
                (Some((c, game, ctl)), Some(w)) => *c == w.target && *game == w.game_window && !ctl.is_finished(),
                (None, None) => true,
                _ => false,
            };
            if !same {
                if let Some((_, _, ctl)) = current.take() {
                    let _ = ctl.stop();
                    status.lock().unwrap().audio_warnings = Default::default();
                    println!("Capture stopped.");
                }
                save.store(false, Ordering::Relaxed);
                if let Some(selected) = &want {
                    let t = selected.target;
                    let name = &selected.name;
                    println!("Recording {name} ({t:?})");
                    {
                        let mut st = status.lock().unwrap();
                        st.state = RecorderState::Recording;
                        st.target = Some(name.clone());
                        st.error = hotkey_error.lock().unwrap().clone();
                    }
                    on_status_change();
                    match start_session(selected, &cfg_snap, save.clone(), status.clone()) {
                        Ok(ctl) => current = Some((t, selected.game_window, ctl)),
                        Err(e) => {
                            eprintln!("could not start capture: {e}");
                            {
                                let mut st = status.lock().unwrap();
                                st.state = RecorderState::Idle;
                                st.target = None;
                                st.error = Some(e.to_string());
                            }
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
                println!("Waiting for a game window (minimized games are paused)...");
                announced_wait = true;
            }
            for _ in 0..20 {
                if stop.load(Ordering::Relaxed) || reload.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        if let Some((_, _, ctl)) = current.take() {
            let _ = ctl.stop();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_selection_uses_primary_monitor_instead_of_the_foreground_window() {
        let cfg = Config { mode: CaptureMode::ScreenCapture, games: Vec::new(), ..Config::default() };
        let selection = desired(&cfg, Some(Target { hwnd: 1, pid: 1 })).expect("primary display");
        let monitor = Monitor::primary().unwrap();
        assert_eq!(selection.target, CaptureTarget::Monitor(monitor.as_raw_hmonitor() as usize));
        assert!(selection.game_window.is_none());
        assert!(!selection.minimized);
        assert_eq!(desired(&cfg, None).unwrap().target, selection.target);
    }

    fn candidate(hwnd: usize, pid: u32, area: u64) -> GameWindow {
        GameWindow { target: Target { hwnd, pid }, name: "game.exe".into(), area, owned: false, minimized: false }
    }

    #[test]
    fn game_names_match_case_insensitively() {
        let g = vec!["VALORANT-Win64-Shipping.exe".to_string()];
        assert!(is_game("valorant-win64-shipping.exe", &g));
        assert!(!is_game("chrome.exe", &g));
    }

    #[test]
    fn alt_tab_and_enumeration_order_do_not_switch_games() {
        let first = candidate(10, 1, 1920 * 1080);
        let second = candidate(20, 2, 2560 * 1440);
        let previous = first.target;
        let windows = [second, first];
        assert_eq!(select_game_window(&windows, Some(previous), Some(99)).unwrap().target, previous);
        assert_eq!(select_game_window(&windows, Some(previous), Some(20)).unwrap().target, previous);
    }

    #[test]
    fn main_window_replaces_splash_and_owned_dialog_does_not_win() {
        let splash = candidate(10, 1, 400 * 300);
        let main = candidate(11, 1, 1920 * 1080);
        let mut dialog = candidate(12, 1, 3840 * 2160);
        dialog.owned = true;
        let previous = splash.target;
        let expected = main.target;
        assert_eq!(select_game_window(&[splash, dialog, main.clone()], Some(previous), None).unwrap().target, expected);
        assert_eq!(select_game_window(&[main], Some(previous), None).unwrap().target, expected);
    }

    #[test]
    fn minimized_game_stays_selected_and_reused_hwnd_changes_identity() {
        let mut minimized = candidate(10, 1, 0);
        minimized.minimized = true;
        let previous = minimized.target;
        let other = candidate(20, 2, 1000 * 1000);
        assert!(select_game_window(&[other.clone(), minimized], Some(previous), Some(20)).unwrap().minimized);
        let reused = candidate(10, 3, 800 * 600);
        assert_eq!(select_game_window(&[other, reused], Some(previous), Some(20)).unwrap().target.pid, 2);
    }

    #[test]
    fn native_game_window_survives_title_changes_minimize_and_restore() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowTextW, ShowWindow, SW_MINIMIZE, SW_SHOWNOACTIVATE};
        let window = crate::test_window::TestWindow::new();
        let exe = std::env::current_exe().unwrap();
        let games = vec![exe.file_name().unwrap().to_string_lossy().into_owned()];
        let initial = find_game_window(&games, None).expect("own test window must be capturable");
        assert_eq!(initial.target.hwnd, window.hwnd);
        assert_eq!(initial.target.pid, std::process::id());
        unsafe { SetWindowTextW(window.handle(), w!("Completely different title - channel name")).unwrap(); }
        assert_eq!(find_game_window(&games, Some(initial.target)).unwrap().target, initial.target);
        unsafe { let _ = ShowWindow(window.handle(), SW_MINIMIZE); }
        assert!(find_game_window(&games, Some(initial.target)).unwrap().minimized);
        unsafe { let _ = ShowWindow(window.handle(), SW_SHOWNOACTIVATE); }
        let restored = find_game_window(&games, Some(initial.target)).unwrap();
        assert_eq!(restored.target, initial.target);
        assert!(!restored.minimized);
        drop(window);
        assert!(find_game_window(&games, Some(initial.target)).is_none());
    }
}
