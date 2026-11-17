//! Updates live only in the library process. The recorder has no updater dependency.
use std::sync::Mutex;
use std::time::{Duration, Instant};

use encore_common::ipc::{self, IpcRequest, IpcResponse};
use serde::Serialize;
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct UpdateState {
    // Serialize checks and installations without holding a blocking mutex over await.
    operation: tauri::async_runtime::Mutex<()>,
    pending: Mutex<Option<Update>>,
}

#[derive(Serialize)]
pub struct AvailableUpdate {
    version: String,
    notes: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum UpdateProgress {
    Downloading {
        downloaded: usize,
        total: Option<u64>,
    },
    Preparing,
    Installing,
}

#[tauri::command]
pub async fn check_for_updates(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<Option<AvailableUpdate>, String> {
    let _operation = state
        .operation
        .try_lock()
        .map_err(|_| "An update operation is already running")?;
    crate::lifecycle::global().ensure_available()?;
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("Could not check for updates: {e}"))?;
    let available = update.as_ref().map(|u| AvailableUpdate {
        version: u.version.clone(),
        notes: u.body.clone(),
    });
    *state.pending.lock().unwrap() = update;
    Ok(available)
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
    on_progress: Channel<UpdateProgress>,
) -> Result<(), String> {
    let _operation = state
        .operation
        .try_lock()
        .map_err(|_| "An update operation is already running")?;
    let selected_version = state
        .pending
        .lock()
        .unwrap()
        .as_ref()
        .map(|u| u.version.clone())
        .ok_or("Check for an update before installing")?;
    let guard = crate::lifecycle::global().begin_update()?;

    // Build a new request with a download timeout rather than inheriting the short check timeout.
    // Recheck so a withdrawn release cannot be installed from stale UI metadata.
    let update = app
        .updater_builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .filter(|u| u.version == selected_version)
        .ok_or("The available release changed. Check for updates again.")?;
    let mut downloaded = 0;
    let mut last_report = Instant::now() - Duration::from_secs(1);
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk;
                if last_report.elapsed() >= Duration::from_millis(150) {
                    let _ = on_progress.send(UpdateProgress::Downloading { downloaded, total });
                    last_report = Instant::now();
                }
            },
            || {},
        )
        .await
        .map_err(|e| format!("Update download or signature verification failed: {e}"))?;

    // Download and signature verification finish before capture is interrupted.
    let _ = on_progress.send(UpdateProgress::Preparing);
    tauri::async_runtime::spawn_blocking(move || {
        guard.wait_for_jobs();
        let resume_path = resume_marker(&app)?;
        if recorder_is_running()? {
            std::fs::create_dir_all(resume_path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&resume_path, b"resume")
                .map_err(|e| format!("Could not preserve recorder state: {e}"))?;
        }
        let recorder_was_running = stop_recorder_for_update()?;
        let _ = on_progress.send(UpdateProgress::Installing);
        // On Windows this launches NSIS and exits the UI; NSIS restarts the new UI.
        if let Err(error) = update.install(bytes) {
            drop(guard);
            if recorder_was_running {
                let resources = app.path().resource_dir().ok();
                if crate::launch_recorder_blocking(resources.as_deref()).is_ok() {
                    let _ = std::fs::remove_file(&resume_path);
                }
            }
            return Err(format!("Could not start the installer: {error}"));
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Update task failed: {e}"))?
}

fn resume_marker(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|p| p.join("resume-recorder-after-update"))
        .map_err(|e| e.to_string())
}

pub fn resume_after_update(app: &AppHandle) {
    let Ok(path) = resume_marker(app) else { return };
    if path.exists() {
        let resources = app.path().resource_dir().ok();
        if crate::launch_recorder_blocking(resources.as_deref()).is_ok() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The recorder holds this named object for its entire lifetime, including save draining.
/// A missing IPC pipe alone does not mean its executable is safe to replace.
fn recorder_is_running() -> Result<bool, String> {
    singleton_exists("Local\\EncoreRecorderSingleton")
}

fn singleton_exists(name: &str) -> Result<bool, String> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND};
    use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &HSTRING::from(name)) } {
        Ok(handle) => {
            unsafe {
                let _ = CloseHandle(handle);
            }
            Ok(true)
        }
        Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(false),
        Err(e) => Err(format!("Could not check recorder shutdown: {e}")),
    }
}

fn stop_recorder_for_update() -> Result<bool, String> {
    if !recorder_is_running()? {
        return Ok(false);
    }
    // Keep the acknowledgement bounded even if the recorder is not responding.
    request_shutdown()?;
    wait_for_recorder_exit(Duration::from_secs(60), recorder_is_running)?;
    Ok(true)
}

fn wait_for_recorder_exit(
    timeout: Duration,
    mut running: impl FnMut() -> Result<bool, String>,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while running()? {
        if Instant::now() >= deadline {
            return Err("The recorder is still finishing saved clips. Installation was not started. Wait and retry.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn request_shutdown() -> Result<(), String> {
    use std::io::{Read, Write};
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{Foundation::HANDLE, System::Pipes::PeekNamedPipe};
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut file = loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(ipc::PIPE_NAME)
        {
            Ok(file) => break file,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                return Err(format!(
                    "Recorder did not respond. Installation was not started: {e}"
                ))
            }
        }
    };
    let request = serde_json::to_string(&IpcRequest::Shutdown).map_err(|e| e.to_string())?;
    writeln!(file, "{request}").map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    loop {
        if Instant::now() >= deadline {
            return Err(
                "Recorder shutdown acknowledgement timed out. Installation was not started.".into(),
            );
        }
        let mut available = 0;
        unsafe {
            PeekNamedPipe(
                HANDLE(file.as_raw_handle()),
                None,
                0,
                None,
                Some(&mut available),
                None,
            )
        }
        .map_err(|e| format!("Could not confirm recorder shutdown: {e}"))?;
        if available == 0 {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        let mut chunk = [0u8; 256];
        let count = file
            .read(&mut chunk[..(available as usize).min(256)])
            .map_err(|e| e.to_string())?;
        response.extend_from_slice(&chunk[..count]);
        if response.contains(&b'\n') {
            return match serde_json::from_slice::<IpcResponse>(&response) {
                Ok(IpcResponse::Ok) => Ok(()),
                other => Err(format!("Recorder refused shutdown: {other:?}")),
            };
        }
        if count == 0 || response.len() > 4096 {
            return Err("Invalid recorder shutdown response. Installation was not started.".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use windows::core::HSTRING;
    use windows::Win32::{Foundation::CloseHandle, System::Threading::CreateMutexW};

    #[test]
    fn updater_waits_for_the_recorder_lifetime_not_just_a_shutdown_acknowledgement() {
        let name = format!("Local\\EncoreUpdateDrainTest_{}", std::process::id());
        let handle = unsafe { CreateMutexW(None, false, &HSTRING::from(&name)).unwrap() };
        let (tx, rx) = mpsc::channel();
        let query_name = name.clone();
        let waiter = std::thread::spawn(move || {
            tx.send(wait_for_recorder_exit(Duration::from_secs(3), || {
                singleton_exists(&query_name)
            }))
            .unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());
        unsafe {
            CloseHandle(handle).unwrap();
        }
        rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
        waiter.join().unwrap();
    }

    #[test]
    fn timeout_or_inaccessible_recorder_aborts_without_killing_it() {
        let name = format!("Local\\EncoreUpdateTimeoutTest_{}", std::process::id());
        let handle = unsafe { CreateMutexW(None, false, &HSTRING::from(&name)).unwrap() };
        let result = wait_for_recorder_exit(Duration::from_millis(20), || singleton_exists(&name));
        assert!(result.unwrap_err().contains("Installation was not started"));
        assert!(singleton_exists(&name).unwrap());
        unsafe {
            CloseHandle(handle).unwrap();
        }
        assert!(!singleton_exists(&name).unwrap());
        assert!(
            wait_for_recorder_exit(Duration::from_secs(1), || Err("access denied".into())).is_err()
        );
    }
}
