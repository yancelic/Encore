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
}

pub fn capabilities() -> PlatformCapabilities {
    PlatformCapabilities {
        os: std::env::consts::OS,
        recording: cfg!(windows),
        application_audio: crate::processes::application_loopback_supported(),
        autostart: cfg!(windows),
        updates: cfg!(windows),
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
    fn preview_does_not_advertise_unimplemented_recording() {
        assert_eq!(capabilities().recording, cfg!(windows));
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
