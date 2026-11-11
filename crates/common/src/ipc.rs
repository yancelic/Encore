//! Named-pipe IPC protocol between `encore-recorder` and `encore-ui`.
use serde::{Deserialize, Serialize};

use crate::CaptureMode;

pub const PIPE_NAME: &str = r"\\.\pipe\EncoreRecorderIpc";
pub const UI_WAKE_PIPE: &str = r"\\.\pipe\EncoreUiWake";

/// Acknowledged UI commands let the launcher distinguish an accepted wake from a
/// window whose process has already committed to exiting.
pub fn send_ui_command(command: &str) -> Result<bool, String> {
    send_ui_command_to_pipe(UI_WAKE_PIPE, command, std::time::Duration::from_millis(300))
}

fn send_ui_command_to_pipe(pipe: &str, command: &str, timeout: std::time::Duration) -> Result<bool, String> {
    use std::io::{Read, Write};
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{Foundation::HANDLE, System::Pipes::PeekNamedPipe};
    if !matches!(command, "show" | "quit") { return Err("invalid UI command".into()); }
    let mut file = std::fs::OpenOptions::new().read(true).write(true).open(pipe).map_err(|e| e.to_string())?;
    // A fresh local connection has a 256-byte server buffer; each control request is five bytes.
    writeln!(file, "{command}").map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + timeout;
    let mut answer = Vec::new();
    loop {
        if std::time::Instant::now() >= deadline { return Err("UI acknowledgement timed out".into()); }
        let mut available = 0;
        unsafe { PeekNamedPipe(HANDLE(file.as_raw_handle()), None, 0, None, Some(&mut available), None) }.map_err(|e| e.to_string())?;
        if available != 0 {
            let mut bytes = [0u8; 64];
            let count = file.read(&mut bytes[..(available as usize).min(64)]).map_err(|e| e.to_string())?;
            if count == 0 { return Ok(false); }
            answer.extend_from_slice(&bytes[..count]);
            if answer.contains(&b'\n') { return Ok(answer == b"ok\n" || answer == b"ok\r\n"); }
            if answer.len() >= 64 { return Err("invalid UI acknowledgement".into()); }
        } else { std::thread::sleep(std::time::Duration::from_millis(5)); }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecorderState {
    Idle,
    Recording,
    Saving,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecorderStatus {
    pub state: RecorderState,
    pub target: Option<String>,
    pub mode: CaptureMode,
    pub fps: u32,
    pub height: u32,
    pub buffer_secs: u32,
    pub hotkey: String,
    pub last_clip: Option<String>,
    pub error: Option<String>,
    /// Game / Chat / Microphone routing problems, independent of recording errors.
    #[serde(default)]
    pub audio_warnings: [Option<String>; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcRequest {
    GetStatus,
    ListAudioDevices,
    ReloadConfig,
    SaveClip,
    SetTrayVisible(bool),
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResponse {
    Status(RecorderStatus),
    AudioDevices(Vec<AudioDeviceInfo>),
    Ok,
    Err(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioDeviceKind {
    Output,
    Input,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub kind: AudioDeviceKind,
    pub is_default: bool,
}

/// Enumerate active output and input audio endpoints using WASAPI.
pub fn list_audio_devices() -> Vec<AudioDeviceInfo> {
    let mut out = Vec::new();
    let _ = wasapi::initialize_mta();
    let Ok(en) = wasapi::DeviceEnumerator::new() else {
        return out;
    };
    for (dir, kind) in [
        (wasapi::Direction::Render, AudioDeviceKind::Output),
        (wasapi::Direction::Capture, AudioDeviceKind::Input),
    ] {
        let def_name = en.get_default_device(dir).ok().and_then(|d| d.get_friendlyname().ok());
        if let Ok(coll) = en.get_device_collection(dir) {
            for i in 0..coll.get_nbr_devices().unwrap_or(0) {
                if let Ok(d) = coll.get_device_at_index(i) {
                    if let Ok(name) = d.get_friendlyname() {
                        let is_default = def_name.as_deref() == Some(name.as_str());
                        out.push(AudioDeviceInfo { name, kind, is_default });
                    }
                }
            }
        }
    }
    out
}

/// Send one synchronous JSON-line IPC request to the given named pipe.
pub fn send_to_pipe(pipe_name: &str, req: &IpcRequest) -> Result<IpcResponse, String> {
    use std::io::{BufRead, BufReader, Write};
    let mut file = None;
    let mut last_err = String::new();
    for _ in 0..10 {
        match std::fs::OpenOptions::new().read(true).write(true).open(pipe_name) {
            Ok(f) => {
                file = Some(f);
                break;
            }
            Err(e) if matches!(e.raw_os_error(), Some(2 | 231 | 232 | 233)) => {
                // A live single-instance server briefly has no pipe (ERROR_FILE_NOT_FOUND)
                // or reports PIPE_BUSY / NO_DATA / PIPE_NOT_CONNECTED during teardown.
                // Retry only opening: after a write, replaying SaveClip could duplicate it.
                last_err = e.to_string();
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            Err(e) => return Err(format!("open ipc pipe: {e}")),
        }
    }
    let mut client = file.ok_or_else(|| format!("pipe unavailable: {last_err}"))?;
    let msg = serde_json::to_string(req).map_err(|e| e.to_string())?;
    writeln!(client, "{msg}").map_err(|e| format!("write ipc request: {e}"))?;
    client.flush().map_err(|e| format!("flush ipc request: {e}"))?;

    let mut line = String::new();
    BufReader::new(client).read_line(&mut line).map_err(|e| format!("read ipc response: {e}"))?;
    serde_json::from_str::<IpcResponse>(line.trim()).map_err(|e| format!("invalid ipc response: {e}"))
}

/// Send one request to the running `encore-recorder` named-pipe server.
pub fn send_request(req: &IpcRequest) -> Result<IpcResponse, String> {
    send_to_pipe(PIPE_NAME, req)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_acknowledgement_distinguishes_wake_close_and_timeout() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::windows::io::FromRawHandle;
        use std::time::Duration;
        use windows::core::HSTRING;
        use windows::Win32::Foundation::ERROR_PIPE_CONNECTED;
        use windows::Win32::Storage::FileSystem::{FlushFileBuffers, PIPE_ACCESS_DUPLEX};
        use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_WAIT};
        for (index, reply) in [Some("ok\n"), Some("closing\n"), None].into_iter().enumerate() {
            let pipe = format!(r"\\.\pipe\EncoreUiAckTest_{}_{}", std::process::id(), index);
            let (ready, started) = std::sync::mpsc::channel();
            let server_name = pipe.clone();
            let server = std::thread::spawn(move || unsafe {
                let handle = CreateNamedPipeW(&HSTRING::from(server_name), PIPE_ACCESS_DUPLEX, PIPE_WAIT, 1, 256, 256, 0, None);
                assert!(!handle.is_invalid());
                ready.send(()).unwrap();
                let connected = ConnectNamedPipe(handle, None);
                assert!(connected.is_ok() || connected.unwrap_err().code().0 as u32 == ERROR_PIPE_CONNECTED.0);
                let file = std::fs::File::from_raw_handle(handle.0);
                let mut reader = BufReader::new(file);
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                assert_eq!(request, "show\n");
                if let Some(reply) = reply {
                    reader.get_mut().write_all(reply.as_bytes()).unwrap();
                    let _ = FlushFileBuffers(handle);
                } else { std::thread::sleep(Duration::from_millis(150)); }
                let _ = DisconnectNamedPipe(handle);
            });
            started.recv_timeout(Duration::from_secs(2)).unwrap();
            let result = send_ui_command_to_pipe(&pipe, "show", Duration::from_millis(50));
            server.join().unwrap();
            match reply {
                Some("ok\n") => assert!(result.unwrap()),
                Some(_) => assert!(!result.unwrap()),
                None => assert!(result.unwrap_err().contains("timed out")),
            }
        }
    }

    #[test]
    fn ipc_roundtrips_json_lines() {
        let req = IpcRequest::ReloadConfig;
        let line = serde_json::to_string(&req).unwrap();
        let back: IpcRequest = serde_json::from_str(&line).unwrap();
        assert!(matches!(back, IpcRequest::ReloadConfig));

        let resp = IpcResponse::Status(RecorderStatus {
            state: RecorderState::Recording,
            target: Some("VALORANT-Win64-Shipping.exe".into()),
            mode: CaptureMode::GameDetection,
            fps: 60,
            height: 1080,
            buffer_secs: 20,
            hotkey: "Ctrl+Alt+K".into(),
            last_clip: None,
            error: None,
            audio_warnings: Default::default(),
        });
        let s = serde_json::to_string(&resp).unwrap();
        let r2: IpcResponse = serde_json::from_str(&s).unwrap();
        match r2 {
            IpcResponse::Status(st) => {
                assert_eq!(st.state, RecorderState::Recording);
                assert_eq!(st.fps, 60);
            }
            _ => panic!("unexpected variant"),
        }
    }
}
