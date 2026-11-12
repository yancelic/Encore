use std::path::Path;

#[derive(Debug, serde::Serialize)]
pub struct CaptureApplication {
    pub exe: String,
    pub title: String,
}

pub fn running_applications() -> Result<Vec<CaptureApplication>, String> {
    Err(encore_common::platform::RECORDING_UNAVAILABLE.into())
}

pub fn executable_filename(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "Invalid application filename".into())
}
