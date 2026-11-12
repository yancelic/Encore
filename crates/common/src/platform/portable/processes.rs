//! Application routing is unavailable until the native capture backend is ready.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent: u32,
    pub exe: String,
}

pub fn snapshot() -> Vec<ProcessInfo> {
    Vec::new()
}
pub fn application_loopback_supported() -> bool {
    false
}

pub fn valid_executable(exe: &str) -> bool {
    !exe.trim().is_empty()
        && exe.trim() == exe
        && !exe
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
}

#[derive(serde::Serialize)]
pub struct ApplicationAudioOptions {
    pub supported: bool,
    pub executables: Vec<String>,
}

pub fn application_audio_options() -> ApplicationAudioOptions {
    ApplicationAudioOptions {
        supported: false,
        executables: Vec::new(),
    }
}
