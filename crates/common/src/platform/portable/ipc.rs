//! Explicit preview errors; no fake recorder is started on unsupported platforms.
use crate::ipc::{AudioDeviceInfo, IpcRequest, IpcResponse};

pub fn send_ui_command(_: &str) -> Result<bool, String> {
    Err(crate::platform::RECORDING_UNAVAILABLE.into())
}

pub fn send_to_pipe(_: &str, _: &IpcRequest) -> Result<IpcResponse, String> {
    Err(crate::platform::RECORDING_UNAVAILABLE.into())
}

pub fn send_request(_: &IpcRequest) -> Result<IpcResponse, String> {
    Err(crate::platform::RECORDING_UNAVAILABLE.into())
}

pub fn list_audio_devices() -> Vec<AudioDeviceInfo> {
    Vec::new()
}
