//! Per-user login startup; no privileged service or system-wide files.
use std::path::{Path, PathBuf};

fn entry_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    return dirs::home_dir().map(|home| home.join("Library/LaunchAgents/com.encore.recorder.plist"))
        .ok_or_else(|| "Home directory is unavailable".into());
    #[cfg(target_os = "linux")]
    return dirs::config_dir().map(|config| config.join("autostart/com.encore.recorder.desktop"))
        .ok_or_else(|| "Configuration directory is unavailable".into());
}

pub fn is_autostart_enabled() -> bool {
    entry_path().is_ok_and(|path| path.is_file())
}

pub fn set_autostart(executable: &Path, enabled: bool) -> Result<(), String> {
    if std::env::var_os("FLATPAK_ID").is_some() {
        return Err("Flatpak login startup requires permission from the desktop background portal".into());
    }
    let path = entry_path()?;
    if !enabled {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        };
    }
    let executable = std::fs::canonicalize(executable).map_err(|e| e.to_string())?;
    let text = executable.to_str().ok_or("Executable path is not valid UTF-8")?;
    #[cfg(target_os = "macos")]
    let content = {
        let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
            .replace('"', "&quot;").replace('\'', "&apos;");
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>com.encore.recorder</string><key>ProgramArguments</key><array><string>{escaped}</string></array><key>RunAtLoad</key><true/><key>ProcessType</key><string>Interactive</string></dict></plist>\n")
    };
    #[cfg(target_os = "linux")]
    let content = {
        if text.chars().any(char::is_control) { return Err("Executable path contains control characters".into()); }
        let escaped = text.replace('\\', "\\\\\\\\").replace('"', "\\\\\\\"")
            .replace('`', "\\\\`").replace('$', "\\\\$").replace('%', "%%");
        format!("[Desktop Entry]\nType=Application\nName=Encore Recorder\nExec=\"{escaped}\"\nTerminal=false\nX-GNOME-Autostart-enabled=true\n")
    };
    std::fs::create_dir_all(path.parent().ok_or("Invalid startup path")?).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())
}
