//! Native application routing, queried through the background recorder.
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
    crate::platform::native_runtime().is_some()
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
    let answer = crate::platform::native_query("audio_applications").unwrap_or_default();
    ApplicationAudioOptions {
        supported: answer["supported"].as_bool().unwrap_or(false),
        executables: answer["applications"].as_array().into_iter().flatten()
            .filter_map(|item| item["value"].as_str().map(str::to_owned)).collect(),
    }
}
