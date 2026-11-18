//! LaunchAgent content, kept platform-independent so packaging paths can be tested anywhere.
use std::path::Path;

fn xml_path(path: &Path) -> Result<String, String> {
    let text = path.to_str().ok_or("Startup path is not valid UTF-8")?;
    if text.chars().any(|c| c.is_control()) {
        return Err("Startup path contains control characters".into());
    }
    Ok(text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        .replace('"', "&quot;").replace('\'', "&apos;"))
}

pub(super) fn launch_agent(executable: &Path, log_path: &Path) -> Result<String, String> {
    let bundle = executable.ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"));
    let arguments = if let Some(bundle) = bundle {
        // Match manual launch: LaunchServices gives the recorder its own privacy
        // identity. -g keeps it in the background; -W keeps the agent alive until exit.
        format!("<string>/usr/bin/open</string><string>-g</string><string>-W</string>\
            <string>-a</string><string>{}</string><string>--stderr</string><string>{}</string>",
            xml_path(bundle)?, xml_path(log_path)?)
    } else {
        // Unbundled development helper.
        format!("<string>{}</string>", xml_path(executable)?)
    };
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
        <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
        <plist version=\"1.0\"><dict>\
        <key>Label</key><string>com.encore.recorder</string>\
        <key>ProgramArguments</key><array>{arguments}</array>\
        <key>RunAtLoad</key><true/>\
        <key>LimitLoadToSessionType</key><string>Aqua</string>\
        <key>ProcessType</key><string>Interactive</string>\
        </dict></plist>\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_agent_opens_inner_recorder_bundle_with_escaped_paths() {
        let text = launch_agent(
            Path::new("/Applications/Encore & Friends.app/Contents/Resources/Encore Recorder.app/Contents/MacOS/encore-recorder"),
            Path::new("/Users/test/Library/Application Support/Encore/recorder.log"),
        ).unwrap();
        assert!(text.contains("<string>/usr/bin/open</string><string>-g</string><string>-W</string>"));
        assert!(text.contains("<string>-a</string><string>/Applications/Encore &amp; Friends.app/Contents/Resources/Encore Recorder.app</string>"));
        assert!(text.contains("<string>--stderr</string><string>/Users/test/Library/Application Support/Encore/recorder.log</string>"));
        assert!(!text.contains("/Contents/MacOS/encore-recorder</string>"));
        assert!(text.contains("<key>RunAtLoad</key><true/>"));
    }

    #[test]
    fn development_agent_can_launch_an_unbundled_helper() {
        let text = launch_agent(Path::new("/tmp/Encore <dev>/encore-recorder"), Path::new("/tmp/recorder.log")).unwrap();
        assert!(text.contains("<array><string>/tmp/Encore &lt;dev&gt;/encore-recorder</string></array>"));
        assert!(!text.contains("/usr/bin/open"));
        assert!(launch_agent(Path::new("/tmp/bad\npath"), Path::new("/tmp/log")).is_err());
    }
}
