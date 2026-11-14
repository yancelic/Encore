//! Toolhelp metadata only: no process handles, memory access or window-title matching.
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent: u32,
    pub exe: String,
}

pub fn snapshot() -> Vec<ProcessInfo> {
    let mut out = Vec::new();
    unsafe {
        let Ok(handle) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(handle, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                out.push(ProcessInfo {
                    pid: entry.th32ProcessID, parent: entry.th32ParentProcessID,
                    exe: String::from_utf16_lossy(&entry.szExeFile[..len]),
                });
                if Process32NextW(handle, &mut entry).is_err() { break; }
            }
        }
        let _ = CloseHandle(handle);
    }
    out
}

pub fn valid_executable(exe: &str) -> bool {
    exe.len() > 4 && exe.to_ascii_lowercase().ends_with(".exe")
        && !exe.chars().any(|c| c.is_control() || r#"\/:*?"<>|"#.contains(c))
        && exe.trim() == exe
}

/// Select a same-executable root, not its launcher (which may own unrelated apps).
/// Keep a live root stable if several independent instances use the same filename.
pub fn application_root(processes: &[ProcessInfo], exe: &str, preferred: Option<u32>) -> Option<u32> {
    let roots: Vec<_> = processes.iter().filter(|p| {
        p.pid != 0 && p.exe.eq_ignore_ascii_case(exe)
            && !processes.iter().any(|parent| parent.pid == p.parent && parent.exe.eq_ignore_ascii_case(exe))
    }).map(|p| p.pid).collect();
    preferred.filter(|pid| roots.contains(pid)).or_else(|| roots.into_iter().min())
}

pub fn application_loopback_supported() -> bool {
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        let mut version = OSVERSIONINFOW { dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
        unsafe { RtlGetVersion(&mut version).is_ok() && version.dwMajorVersion >= 10 && version.dwBuildNumber >= 20348 }
    })
}

#[derive(serde::Serialize)]
pub struct ApplicationAudioOptions {
    pub supported: bool,
    pub executables: Vec<String>,
}

pub fn application_audio_options() -> ApplicationAudioOptions {
    let mut executables: Vec<_> = snapshot().into_iter().map(|p| p.exe).filter(|e| valid_executable(e)).collect();
    executables.sort_by_key(|s| s.to_ascii_lowercase());
    executables.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    ApplicationAudioOptions { supported: application_loopback_supported(), executables }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(pid: u32, parent: u32, exe: &str) -> ProcessInfo { ProcessInfo { pid, parent, exe: exe.into() } }

    #[test]
    fn selects_root_stays_stable_and_reconnects_after_restart() {
        let first = vec![p(1, 0, "launcher.exe"), p(20, 1, "Chat.exe"), p(10, 20, "chat.EXE"), p(30, 1, "chat.exe")];
        assert_eq!(application_root(&first, "CHAT.exe", None), Some(20));
        assert_eq!(application_root(&first, "chat.exe", Some(30)), Some(30));
        let restarted = vec![p(1, 0, "launcher.exe"), p(60, 1, "Chat.exe"), p(61, 60, "Chat.exe")];
        assert_eq!(application_root(&restarted, "chat.exe", Some(20)), Some(60));
        assert_eq!(application_root(&[], "chat.exe", Some(60)), None);
    }

    #[test]
    fn accepts_filenames_not_window_titles_or_paths() {
        assert!(valid_executable("Some Chat App.exe"));
        for name in ["", "Discord - channel", "C:\\Chat.exe", "../Chat.exe", "chat.exe\n", "*.exe"] {
            assert!(!valid_executable(name), "{name:?}");
        }
    }
}
