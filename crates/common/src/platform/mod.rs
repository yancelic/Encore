//! Shared platform services. Capture backends report their own capabilities.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub const RECORDING_UNAVAILABLE: &str = "Recording is not available in this platform preview yet. You can browse, edit and export existing clips.";

#[derive(Debug, serde::Serialize)]
pub struct PlatformCapabilities {
    pub os: &'static str,
    pub recording: bool,
    pub application_audio: bool,
    pub autostart: bool,
    pub updates: bool,
    pub portal_capture: bool,
}

pub fn capabilities() -> PlatformCapabilities {
    PlatformCapabilities {
        os: std::env::consts::OS,
        recording: cfg!(windows) || native_runtime().is_some(),
        application_audio: crate::processes::application_loopback_supported(),
        autostart: cfg!(windows) || native_runtime().is_some(),
        updates: cfg!(windows),
        portal_capture: cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some(),
    }
}

/// Preserve the existing Windows location so upgrades retain config and library data.
pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    return std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Encore");
    #[cfg(not(windows))]
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
        .join("Encore")
}

pub fn videos_dir() -> PathBuf {
    #[cfg(windows)]
    return std::env::var_os("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Videos"))
        .unwrap_or_else(|| PathBuf::from("."));
    #[cfg(not(windows))]
    dirs::video_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Videos"))
}

static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn set_resource_dir(path: PathBuf) {
    let _ = RESOURCE_DIR.set(path);
}

pub fn recorder_path(resources: Option<&Path>) -> Option<PathBuf> {
    let name = if cfg!(windows) { "encore-recorder.exe" } else { "encore-recorder" };
    for root in resources.into_iter().chain(RESOURCE_DIR.get().map(PathBuf::as_path)) {
        for relative in [PathBuf::from(name), Path::new("Encore Recorder.app/Contents/MacOS").join(name)] {
            let candidate = root.join(relative);
            if candidate.is_file() { return Some(candidate); }
        }
    }
    if let Ok(executable) = std::env::current_exe() {
        for ancestor in executable.ancestors().skip(1) {
            for relative in [PathBuf::from(name), Path::new("resources").join(name),
                Path::new("target/release").join(name), Path::new("target/debug").join(name),
                Path::new("../Resources/Encore Recorder.app/Contents/MacOS").join(name)] {
                let candidate = ancestor.join(relative);
                if candidate.is_file() { return Some(candidate); }
            }
        }
    }
    None
}

/// Query the running native recorder, or a short-lived helper which never starts capture.
#[cfg(unix)]
pub fn native_query(action: &str) -> Result<serde_json::Value, String> {
    use crate::ipc::{self, IpcRequest, IpcResponse, NativeCommand};
    let request = match action {
        "applications" => IpcRequest::Native(NativeCommand::ListCaptureApplications),
        "audio_applications" => IpcRequest::Native(NativeCommand::ListAudioApplications),
        "audio_devices" => IpcRequest::ListAudioDevices,
        "diagnostics" => IpcRequest::Native(NativeCommand::GetDiagnostics),
        _ => return Err("Unsupported native query".into()),
    };
    match ipc::send_request(&request) {
        Ok(IpcResponse::NativeData(value)) => return Ok(value),
        Ok(IpcResponse::AudioDevices(devices)) => return Ok(serde_json::json!({"devices": devices})),
        Ok(IpcResponse::Err(error)) => return Err(error),
        _ => {},
    }
    native_helper_request(&["--query", action])
}

#[cfg(unix)]
pub fn native_helper_request(arguments: &[&str]) -> Result<serde_json::Value, String> {
    let executable = recorder_path(None).ok_or("Native recorder helper is missing")?;
    let output = Command::new(executable).args(arguments)
        .stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let log = String::from_utf8_lossy(&output.stderr);
        let error = log.lines().rev().find_map(|line| line.strip_prefix("Encore recorder: ")).unwrap_or("The native helper failed; check the recorder log");
        return Err(error.chars().take(512).collect());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let response = text.lines().rev().find_map(|line| line.strip_prefix("ENCORE_QUERY "))
        .ok_or("Native query did not return a framed response")?;
    serde_json::from_str(response).map_err(|e| format!("Invalid native query response: {e}"))
}

/// Locate the bundled native engine without depending on an installed OBS frontend.
pub fn native_runtime() -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    let library = if cfg!(target_os = "macos") {
        "libencore_capture.dylib"
    } else {
        "libencore_capture.so"
    };
    let valid = |path: &Path| path.join("lib").join(library).is_file();
    if let Some(path) = std::env::var_os("ENCORE_NATIVE_RUNTIME").map(PathBuf::from) {
        if valid(&path) {
            return Some(path);
        }
    }
    if let Some(resources) = RESOURCE_DIR.get() {
        let path = resources.join("obs-runtime");
        if valid(&path) {
            return Some(path);
        }
    }
    if let Ok(executable) = std::env::current_exe() {
        for ancestor in executable.ancestors().skip(1) {
            if valid(ancestor) { return Some(ancestor.to_path_buf()); }
            for relative in [
                "obs-runtime",
                "resources/obs-runtime",
                "tools/obs-runtime",
                "../Resources/obs-runtime",
            ] {
                let path = ancestor.join(relative);
                if valid(&path) {
                    return Some(path);
                }
            }
        }
    }
    None
}

pub fn ffmpeg_path() -> PathBuf {
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    if let Some(resources) = RESOURCE_DIR.get() {
        let candidate = resources.join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors().skip(1) {
            for relative in [
                PathBuf::from(name),
                Path::new("tools").join(name),
                Path::new("resources").join(name),
                Path::new("resources/tools").join(name),
                Path::new("../Resources").join(name),
            ] {
                let candidate = ancestor.join(relative);
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    PathBuf::from(name)
}

/// Hide helper console windows on Windows; use ordinary process spawning elsewhere.
pub trait CommandExt {
    fn background(&mut self) -> &mut Self;
}

impl CommandExt for Command {
    fn background(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as WindowsCommandExt;
            self.creation_flags(0x0800_0000);
        }
        self
    }
}

pub fn reveal_path(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("Path does not exist: {}", path.display()));
    }
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("explorer.exe");
        if path.is_dir() {
            command.arg(path);
        } else {
            command.arg(format!("/select,{}", path.display()));
        }
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        if path.is_file() {
            command.arg("-R");
        }
        command.arg(path);
        command
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(if path.is_dir() {
            path
        } else {
            path.parent().ok_or("Missing parent directory")?
        });
        command
    };
    command
        .background()
        .spawn()
        .map_err(|e| format!("Could not open the file manager: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_requires_the_platform_runtime() {
        assert_eq!(capabilities().recording, cfg!(windows) || native_runtime().is_some());
        assert_eq!(capabilities().updates, cfg!(windows));
    }

    #[test]
    fn file_manager_rejects_nonexistent_paths() {
        let missing = std::env::temp_dir().join(format!("encore-missing-{}", std::process::id()));
        assert!(reveal_path(&missing)
            .unwrap_err()
            .contains("does not exist"));
    }
}
