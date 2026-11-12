//! Local named-pipe IPC server allowing `encore-ui` to query status, trigger clips, or reload config.
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::FromRawHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use crate::audio;

use encore_common::ipc::{IpcRequest, IpcResponse, RecorderStatus, PIPE_NAME};
use windows::Win32::Foundation::{ERROR_PIPE_CONNECTED, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, WPARAM};
use windows::Win32::Storage::FileSystem::{FlushFileBuffers, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_WAIT,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
use windows::core::HSTRING;

pub const WM_IPC_SAVE: u32 = WM_APP + 10;
pub const WM_IPC_RELOAD: u32 = WM_APP + 11;
pub const WM_IPC_QUIT: u32 = WM_APP + 12;
pub const WM_IPC_SET_TRAY: u32 = WM_APP + 13;

/// Channel passed via `WM_IPC_RELOAD` so the main message loop replies synchronously after re-registering hotkey.
pub type ReloadAck = Mutex<Option<SyncSender<Result<(), String>>>>;

pub struct IpcShared {
    pub status: Arc<Mutex<RecorderStatus>>,
    pub reload_ack: ReloadAck,
}

fn serve_pipe(pipe_name: &str, hwnd_raw: isize, shared: &IpcShared, stop: &AtomicBool) {
    let hname = HSTRING::from(pipe_name);
    while !stop.load(Ordering::Relaxed) {
        let handle: HANDLE = unsafe {
            CreateNamedPipeW(
                &hname,
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                4,
                4096,
                4096,
                0,
                None,
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_invalid() {
            thread::sleep(std::time::Duration::from_millis(100));
            continue;
        }
        // Keep the same instance alive across clients. After DisconnectNamedPipe,
        // Windows can already allow a new client to connect, before the next
        // ConnectNamedPipe call. Dropping/recreating the handle in that interval
        // disconnects that new client after its open succeeded (ERROR_PIPE_NOT_CONNECTED).
        // SAFETY: this File exclusively owns the server handle until shutdown.
        let mut server = unsafe { File::from_raw_handle(handle.0) };
        while !stop.load(Ordering::Relaxed) {
            let connected = unsafe {
                match ConnectNamedPipe(handle, None) {
                    Ok(()) => true,
                    Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult(),
                }
            };
            if !connected || stop.load(Ordering::Relaxed) {
                unsafe {
                    let _ = DisconnectNamedPipe(handle);
                }
                thread::sleep(std::time::Duration::from_millis(15));
                continue;
            }

            let mut reader = BufReader::new(&mut server);
            let mut line = String::new();
            if reader.read_line(&mut line).is_ok() && !line.trim().is_empty() {
                let resp = match serde_json::from_str::<IpcRequest>(line.trim()) {
                    Ok(IpcRequest::GetStatus) => IpcResponse::Status(shared.status.lock().unwrap().clone()),
                    Ok(IpcRequest::ListAudioDevices) => IpcResponse::AudioDevices(audio::list_devices()),
                    Ok(IpcRequest::SaveClip) => {
                        if hwnd_raw != 0 {
                            unsafe {
                                let _ = PostMessageW(Some(HWND(hwnd_raw as *mut _)), WM_IPC_SAVE, WPARAM(0), LPARAM(0));
                            }
                        }
                        IpcResponse::Ok
                    }
                    Ok(IpcRequest::ReloadConfig) => {
                        if hwnd_raw != 0 {
                            let (tx, rx) = mpsc::sync_channel(1);
                            *shared.reload_ack.lock().unwrap() = Some(tx);
                            unsafe {
                                let _ = PostMessageW(Some(HWND(hwnd_raw as *mut _)), WM_IPC_RELOAD, WPARAM(0), LPARAM(0));
                            }
                            match rx.recv_timeout(std::time::Duration::from_secs(3)) {
                                Ok(Ok(())) => IpcResponse::Ok,
                                Ok(Err(e)) => IpcResponse::Err(e),
                                Err(_) => IpcResponse::Err("reload timed out".into()),
                            }
                        } else {
                            IpcResponse::Ok
                        }
                    }
                    Ok(IpcRequest::SetTrayVisible(visible)) => {
                        if hwnd_raw != 0 {
                            unsafe {
                                let _ = PostMessageW(
                                    Some(HWND(hwnd_raw as *mut _)),
                                    WM_IPC_SET_TRAY,
                                    WPARAM(usize::from(visible)),
                                    LPARAM(0),
                                );
                            }
                        }
                        IpcResponse::Ok
                    }
                    Ok(IpcRequest::Shutdown) => {
                        if hwnd_raw != 0 {
                            unsafe {
                                let _ = PostMessageW(Some(HWND(hwnd_raw as *mut _)), WM_IPC_QUIT, WPARAM(0), LPARAM(0));
                            }
                        }
                        IpcResponse::Ok
                    }
                    Err(e) => IpcResponse::Err(format!("invalid ipc request: {e}")),
                };
                let mut out = serde_json::to_string(&resp).unwrap_or_else(|_| "{\"Err\":\"encode\"}".into());
                out.push('\n');
                let file_mut = reader.get_mut();
                let _ = file_mut.write_all(out.as_bytes());
                let _ = file_mut.flush();
                unsafe {
                    let _ = FlushFileBuffers(handle);
                }
            }
            // Also disconnect empty/invalid requests, so one departed client cannot
            // prevent the persistent instance from accepting the next connection.
            unsafe { let _ = DisconnectNamedPipe(handle); }
        }
    }
}

/// Start the IPC named-pipe server on a background thread.
pub fn start(hwnd: HWND, shared: Arc<IpcShared>, stop: Arc<AtomicBool>) {
    let hwnd_raw = hwnd.0 as isize;
    thread::spawn(move || serve_pipe(PIPE_NAME, hwnd_raw, &shared, &stop));
}

#[cfg(test)]
mod tests {
    use super::*;
    use encore_common::CaptureMode;
    use encore_common::ipc::RecorderState;
    use std::fs::OpenOptions;

    #[test]
    fn named_pipe_serves_consecutive_requests_without_disconnecting_new_clients() {
        let pipe = format!(r"\\.\pipe\EncoreTestPipe_{}", std::process::id());
        let shared = Arc::new(IpcShared {
            status: Arc::new(Mutex::new(RecorderStatus {
                state: RecorderState::Idle,
                target: None,
                mode: CaptureMode::GameDetection,
                fps: 60,
                height: 1080,
                buffer_secs: 60,
                hotkey: "Ctrl+Alt+K".into(),
                last_clip: None,
                error: None,
                audio_warnings: Default::default(),
            })),
            reload_ack: Mutex::new(None),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let p_srv = pipe.clone();
        let sh_srv = shared.clone();
        let st_srv = stop.clone();
        let h = thread::spawn(move || serve_pipe(&p_srv, 0, &sh_srv, &st_srv));

        thread::sleep(std::time::Duration::from_millis(80));
        for _ in 0..1000 {
            let resp = encore_common::ipc::send_to_pipe(&pipe, &IpcRequest::GetStatus).expect("pipe request");
            match resp {
                IpcResponse::Status(s) => {
                    assert_eq!(s.state, RecorderState::Idle);
                    assert_eq!(s.hotkey, "Ctrl+Alt+K");
                }
                _ => panic!("expected status"),
            }
        }
        stop.store(true, Ordering::Relaxed);
        // Wake blocked ConnectNamedPipe cleanly (retry briefly if server is between iterations).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !h.is_finished() && std::time::Instant::now() < deadline {
            let _ = OpenOptions::new().read(true).write(true).open(&pipe);
            thread::sleep(std::time::Duration::from_millis(15));
        }
        let _ = h.join();
    }
}
