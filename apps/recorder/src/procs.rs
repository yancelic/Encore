//! Process-name lookup that never opens a handle to another process.
//!
//! A toolhelp snapshot (the same data Task Manager lists) gives pid -> exe name without
//! `OpenProcess`, so Encore never requests read/query access on a game process (anti-cheat safe).
use std::collections::HashMap;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

/// Snapshot of running processes: pid -> executable file name.
pub fn snapshot() -> HashMap<u32, String> {
    let mut map = HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return map };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                map.insert(entry.th32ProcessID, String::from_utf16_lossy(&entry.szExeFile[..len]));
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_contains_this_process() {
        let me = std::process::id();
        let procs = snapshot();
        assert!(procs.get(&me).is_some_and(|n| n.to_lowercase().contains("encore")), "{:?}", procs.get(&me));
    }
}
