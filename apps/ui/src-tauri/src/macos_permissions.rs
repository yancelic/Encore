//! macOS privacy permissions for the recorder helper.
//!
//! macOS attributes a privacy request to the *responsible* process. A helper spawned
//! directly by the library inherits the library as responsible process, while the
//! login agent starts it on its own. Grants were therefore split between "Encore"
//! and "Encore Recorder" depending on how the recorder started. Launching the helper
//! bundle through LaunchServices makes "Encore Recorder" the single owner of screen,
//! audio, microphone and input monitoring access in every case.
use encore_common::ipc::{self, IpcRequest, IpcResponse, NativeCommand};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const RECORDER_BUNDLE_ID: &str = "com.encore.recorder";
pub const LIBRARY_BUNDLE_ID: &str = "com.encore.clipper";
const KINDS: [&str; 3] = ["screen", "microphone", "input_monitoring"];

pub fn recorder_bundle(executable: &Path) -> Option<PathBuf> {
    executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .map(Path::to_path_buf)
}

fn recorder_online() -> bool {
    matches!(ipc::send_request(&IpcRequest::GetStatus), Ok(IpcResponse::Status(_)))
}

pub fn launch_recorder_bundle(bundle: &Path, log_path: &Path) -> Result<(), String> {
    let output = Command::new("/usr/bin/open")
        .arg("-g")
        .arg("-a")
        .arg(bundle)
        .arg("--stderr")
        .arg(log_path)
        .output()
        .map_err(|e| format!("Failed to start {}: {e}", bundle.display()))?;
    if !output.status.success() {
        return Err(format!(
            "macOS could not open {}: {}",
            bundle.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    for _ in 0..100 {
        if recorder_online() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("Recorder launched but did not respond. Details: {}", log_path.display()))
}

fn query(command: NativeCommand) -> Result<Value, String> {
    match ipc::send_request(&IpcRequest::Native(command)) {
        Ok(IpcResponse::NativeData(mut value)) => {
            value["running"] = json!(true);
            Ok(value)
        }
        Ok(IpcResponse::Err(error)) => Err(error),
        Ok(_) => Err("Unexpected recorder response".into()),
        Err(error) => Err(error),
    }
}

pub fn status() -> Result<Value, String> {
    if !recorder_online() {
        return Ok(json!({ "running": false }));
    }
    query(NativeCommand::GetPermissions)
}

fn ensure_recorder(resources: Option<&Path>) -> Result<(), String> {
    if recorder_online() {
        return Ok(());
    }
    crate::launch_recorder_blocking(resources)
}

pub fn request(resources: Option<&Path>, kind: &str) -> Result<Value, String> {
    if !KINDS.contains(&kind) {
        return Err(format!("Unknown permission: {kind}"));
    }
    ensure_recorder(resources)?;
    query(NativeCommand::RequestPermission(kind.into()))
}

pub fn open_settings(kind: &str) -> Result<(), String> {
    let pane = match kind {
        "screen" => "Privacy_ScreenCapture",
        "microphone" => "Privacy_Microphone",
        "input_monitoring" => "Privacy_ListenEvent",
        _ => return Err(format!("Unknown permission: {kind}")),
    };
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{pane}");
    let status = Command::new("/usr/bin/open").arg(url).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err("System Settings could not be opened".into()) }
}

/// Remove every Encore privacy decision, including stale entries left by earlier
/// ad-hoc builds and grants that were attributed to the library app, then restart
/// the recorder so its next request shows fresh system prompts.
pub fn reset(resources: Option<&Path>) -> Result<Value, String> {
    crate::stop_recorder_blocking();
    let mut failures = Vec::new();
    for bundle in [RECORDER_BUNDLE_ID, LIBRARY_BUNDLE_ID] {
        let output = Command::new("/usr/bin/tccutil")
            .args(["reset", "All", bundle])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            failures.push(format!("{bundle}: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
    }
    // tccutil fails for a bundle ID that LaunchServices has never seen; that is
    // only an error when neither identity could be reset.
    if failures.len() == 2 {
        return Err(format!("Could not reset permissions. {}", failures.join("; ")));
    }
    crate::launch_recorder_blocking(resources)?;
    status()
}
