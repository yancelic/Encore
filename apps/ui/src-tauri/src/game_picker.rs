//! On-demand window titles + Toolhelp filenames; no game process handles or polling.
use std::collections::HashMap;

use encore_common::processes::{self, ProcessInfo};
use serde::Serialize;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetWindow, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, GW_OWNER,
};

#[derive(Debug, Serialize)]
pub struct CaptureApplication {
    exe: String,
    title: String,
}

struct Candidate {
    pid: u32,
    title: String,
    area: u64,
    owned: bool,
}

unsafe extern "system" fn collect_window(hwnd: HWND, data: LPARAM) -> windows::core::BOOL {
    if !IsWindowVisible(hwnd).as_bool() {
        return true.into();
    }
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    // Never send WM_GETTEXT to Encore's own UI thread from the worker.
    if pid == 0 || pid == std::process::id() {
        return true.into();
    }
    let mut rect = RECT::default();
    if GetClientRect(hwnd, &mut rect).is_err()
        || rect.right - rect.left < 200
        || rect.bottom - rect.top < 200
    {
        return true.into();
    }
    let mut text = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut text);
    if len > 0 {
        let candidates = &mut *(data.0 as *mut Vec<Candidate>);
        candidates.push(Candidate {
            pid,
            title: String::from_utf16_lossy(&text[..len as usize]),
            area: (rect.right - rect.left) as u64 * (rect.bottom - rect.top) as u64,
            owned: GetWindow(hwnd, GW_OWNER).is_ok(),
        });
    }
    true.into()
}

fn select_applications(
    candidates: Vec<Candidate>,
    processes: &[ProcessInfo],
) -> Vec<CaptureApplication> {
    let filenames: HashMap<_, _> = processes.iter().map(|p| (p.pid, &p.exe)).collect();
    let mut selected: HashMap<String, (String, Candidate)> = HashMap::new();
    for candidate in candidates {
        let Some(exe) = filenames.get(&candidate.pid) else {
            continue;
        };
        let key = exe.to_ascii_lowercase();
        if !processes::valid_executable(exe)
            || candidate.title.trim().is_empty()
            || matches!(
                key.as_str(),
                "encore-ui.exe"
                    | "encore-recorder.exe"
                    | "msedgewebview2.exe"
                    | "explorer.exe"
                    | "searchhost.exe"
                    | "shellexperiencehost.exe"
                    | "startmenuexperiencehost.exe"
                    | "textinputhost.exe"
            )
        {
            continue;
        }
        let rank = |w: &Candidate| (!w.owned, w.area, std::cmp::Reverse(w.title.clone()));
        if selected
            .get(&key)
            .is_none_or(|(_, previous)| rank(&candidate) > rank(previous))
        {
            selected.insert(key, ((*exe).clone(), candidate));
        }
    }
    let mut result: Vec<_> = selected
        .into_values()
        .map(|(exe, window)| CaptureApplication {
            exe,
            title: window.title,
        })
        .collect();
    result.sort_by_key(|app| (app.title.to_lowercase(), app.exe.to_ascii_lowercase()));
    result
}

pub fn running_applications() -> Result<Vec<CaptureApplication>, String> {
    let processes = processes::snapshot();
    let mut candidates = Vec::<Candidate>::new();
    unsafe {
        EnumWindows(
            Some(collect_window),
            LPARAM(&mut candidates as *mut _ as isize),
        )
    }
    .map_err(|e| format!("Could not list running applications: {e}"))?;
    Ok(select_applications(candidates, &processes))
}

pub fn executable_filename(path: &std::path::Path) -> Result<String, String> {
    let exe = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Choose a Windows game executable (.exe).")?;
    if !processes::valid_executable(exe) {
        return Err("Choose a Windows game executable (.exe).".into());
    }
    Ok(exe.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_uses_real_filenames_deduplicates_and_prefers_the_main_window() {
        let processes = vec![
            ProcessInfo {
                pid: 1,
                parent: 0,
                exe: "NewGame-Win64-Shipping.exe".into(),
            },
            ProcessInfo {
                pid: 2,
                parent: 0,
                exe: "NEWGAME-WIN64-SHIPPING.EXE".into(),
            },
            ProcessInfo {
                pid: 3,
                parent: 0,
                exe: "explorer.exe".into(),
            },
        ];
        let window = |pid, title: &str, area, owned| Candidate {
            pid,
            title: title.into(),
            area,
            owned,
        };
        let apps = select_applications(
            vec![
                window(1, "Crash dialog", 100000, true),
                window(1, "New Game", 50000, false),
                window(2, "Splash", 10000, false),
                window(3, "Downloads", 100000, false),
                window(99, "Departed process", 100000, false),
            ],
            &processes,
        );
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].exe, "NewGame-Win64-Shipping.exe");
        assert_eq!(apps[0].title, "New Game");
    }

    #[test]
    fn browsing_remembers_the_filename_instead_of_a_machine_specific_path() {
        assert_eq!(
            executable_filename(std::path::Path::new(r"C:\Games\My Game\Game.exe")).unwrap(),
            "Game.exe"
        );
        assert!(executable_filename(std::path::Path::new(r"C:\Games\Game.lnk")).is_err());
    }
}
