//! Named-pipe IPC protocol between `encore-recorder` and `encore-ui`.
use serde::{Deserialize, Serialize};

use crate::CaptureMode;

pub const PIPE_NAME: &str = r"\\.\pipe\EncoreRecorderIpc";
pub const UI_WAKE_PIPE: &str = r"\\.\pipe\EncoreUiWake";

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
            Err(e) if e.raw_os_error() == Some(231) => {
                // ERROR_PIPE_BUSY: server is between client connections; brief retry.
                last_err = e.to_string();
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut client = file.ok_or_else(|| format!("pipe busy: {last_err}"))?;
    let msg = serde_json::to_string(req).map_err(|e| e.to_string())?;
    writeln!(client, "{msg}").map_err(|e| e.to_string())?;
    client.flush().map_err(|e| e.to_string())?;

    let mut line = String::new();
    BufReader::new(client).read_line(&mut line).map_err(|e| e.to_string())?;
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
