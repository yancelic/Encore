//! Joins finished segments into a clip using FFmpeg stream copy (never runs while recording frames).
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub use encore_common::ffmpeg_path;

/// Concatenate `segments` (oldest first), preserve all video/audio streams, and keep the last `secs` seconds.
pub fn save_clip(segments: &[PathBuf], secs: u32, out: &Path, game_name: Option<&str>) -> Result<(), String> {
    if segments.is_empty() {
        return Err("no finished segments yet".into());
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let list = out.with_extension("txt");
    let body: String = segments
        .iter()
        .map(|p| format!("file '{}'\n", p.to_string_lossy().replace('\\', "/")))
        .collect();
    std::fs::write(&list, body).map_err(|e| e.to_string())?;

    let joined = out.with_extension("joined.mp4");
    let ff = ffmpeg_path();
    let run = |args: &[&std::ffi::OsStr]| -> Result<(), String> {
        let st = Command::new(&ff)
            .creation_flags(CREATE_NO_WINDOW)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("could not run ffmpeg: {e}"))?;
        if st.success() { Ok(()) } else { Err(format!("ffmpeg failed: {st}")) }
    };
    use std::ffi::OsStr;
    run(&[
        OsStr::new("-y"), OsStr::new("-f"), OsStr::new("concat"), OsStr::new("-safe"), OsStr::new("0"),
        OsStr::new("-i"), list.as_os_str(), OsStr::new("-map"), OsStr::new("0"),
        OsStr::new("-c"), OsStr::new("copy"), joined.as_os_str(),
    ])?;
    // Trim to the last `secs` seconds while keeping all 3 audio tracks + game metadata.
    let neg = format!("-{secs}");
    let comment = game_name.map(|g| format!("comment=encore_game:{g}"));
    let mut trim_args: Vec<&OsStr> = vec![
        OsStr::new("-y"),
        OsStr::new("-sseof"),
        OsStr::new(&neg),
        OsStr::new("-i"),
        joined.as_os_str(),
        OsStr::new("-map"),
        OsStr::new("0"),
        OsStr::new("-c"),
        OsStr::new("copy"),
        OsStr::new("-movflags"),
        OsStr::new("+faststart"),
    ];
    if let Some(ref c) = comment {
        trim_args.push(OsStr::new("-metadata"));
        trim_args.push(OsStr::new(c));
    }
    trim_args.push(out.as_os_str());
    let r = run(&trim_args);
    let _ = std::fs::remove_file(&list);
    let _ = std::fs::remove_file(&joined);
    r
}
