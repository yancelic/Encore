use std::path::Path;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CaptureApplication {
    pub exe: String,
    pub title: String,
}

pub fn running_applications() -> Result<Vec<CaptureApplication>, String> {
    let answer = encore_common::platform::native_query("applications")?;
    let mut applications: Vec<CaptureApplication> = serde_json::from_value(answer["applications"].clone()).map_err(|e| e.to_string())?;
    applications.sort_by(|a, b| a.exe.cmp(&b.exe));
    applications.dedup_by(|a, b| a.exe == b.exe);
    Ok(applications)
}

pub fn executable_filename(path: &Path) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    if path.extension().is_some_and(|extension| extension == "app") {
        let output = std::process::Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", "Print :CFBundleIdentifier"]).arg(path.join("Contents/Info.plist"))
            .output().map_err(|e| e.to_string())?;
        let identifier = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if output.status.success() && !identifier.is_empty() { return Ok(identifier); }
        return Err("The selected application has no bundle identifier".into());
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "Invalid application filename".into())
}
