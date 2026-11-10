//! Windows per-user Run-key autostart management (HKCU\Software\Microsoft\Windows\CurrentVersion\Run).
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "EncoreRecorder";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Enable or disable launching `exe_path` on Windows sign-in.
pub fn set_autostart(exe_path: &Path, enabled: bool) -> Result<(), String> {
    let mut cmd = Command::new("reg");
    cmd.creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if enabled {
        let val = format!("\"{}\"", exe_path.display());
        cmd.args(["add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d", &val, "/f"]);
    } else {
        cmd.args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"]);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() || !enabled {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Check whether the per-user autostart registry entry currently exists.
pub fn is_autostart_enabled() -> bool {
    Command::new("reg")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["query", RUN_KEY, "/v", VALUE_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
