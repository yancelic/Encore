//! File clipboard integration for native desktop previews.
use std::path::Path;

pub fn copy_file_to_clipboard(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("Clip does not exist: {}", path.display()));
    }
    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        use std::process::{Command, Stdio};
        // The path is passed as argv, never interpolated into AppleScript source.
        let mut child = Command::new("/usr/bin/osascript")
            .args(["-", &path.to_string_lossy()])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not open the clipboard: {e}"))?;
        child
            .stdin
            .take()
            .ok_or("Missing AppleScript input")?
            .write_all(
                b"on run argv\nset the clipboard to (POSIX file (item 1 of argv))\nend run\n",
            )
            .map_err(|e| e.to_string())?;
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if output.status.success() {
            return Ok(());
        }
        return Err(format!(
            "Could not copy the clip: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    #[cfg(not(target_os = "macos"))]
    Err("File clipboard sharing is not available in this preview yet. Use Show in folder to share the exported clip.".into())
}
