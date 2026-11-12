//! Shared recorder protocol; the transport is selected separately for each platform.
use serde::{Deserialize, Serialize};
use crate::CaptureMode;

#[cfg(windows)]
#[path = "platform/windows/ipc.rs"]
mod transport;
#[cfg(not(windows))]
#[path = "platform/unix_ipc.rs"]
mod transport;
pub use transport::*;

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
    #[cfg(unix)]
    Native(NativeCommand),
}

#[cfg(unix)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NativeCommand {
    ListCaptureApplications,
    ListAudioApplications,
    SelectCaptureSource,
    GetDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResponse {
    Status(RecorderStatus),
    AudioDevices(Vec<AudioDeviceInfo>),
    Ok,
    Err(String),
    #[cfg(unix)]
    NativeData(serde_json::Value),
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
