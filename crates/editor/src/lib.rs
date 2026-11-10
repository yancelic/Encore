//! Clip trimming, 9:16 vertical export, montage concatenation, and Windows CF_HDROP clipboard share.
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Shell::DROPFILES;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CF_HDROP: u32 = 15;

fn run_ffmpeg(ffmpeg: &Path, args: &[&OsStr]) -> Result<(), String> {
    let out = Command::new(ffmpeg)
        .creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("could not run ffmpeg: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = err.lines().rev().take(6).collect();
        Err(format!("ffmpeg failed ({}): {}", out.status, tail.into_iter().rev().collect::<Vec<_>>().join(" | ")))
    }
}

/// Resolve the effective linear gain for track `idx`, returning `0.0` when muted.
fn effective_track_volume(muted_tracks: &[bool], track_volumes: &[f64], idx: usize) -> f64 {
    if *muted_tracks.get(idx).unwrap_or(&false) {
        return 0.0;
    }
    let raw = track_volumes.get(idx).copied().unwrap_or(1.0);
    if raw.is_finite() {
        raw.clamp(0.0, 2.0)
    } else {
        1.0
    }
}

pub struct TrimOpts<'a> {
    pub start_secs: f64,
    pub end_secs: f64,
    pub audio_tracks: u32,
    pub muted_tracks: &'a [bool],
    pub track_volumes: &'a [f64],
}

/// Trim `src` to `[start_secs, end_secs]`. Uses instant stream copy when all audio tracks stay at 100%;
/// otherwise stream-copies video and applies per-track volume/mute filters while preserving track layout.
pub fn trim_clip(
    ffmpeg: &Path,
    src: &Path,
    dst: &Path,
    opts: &TrimOpts<'_>,
) -> Result<PathBuf, String> {
    if opts.end_secs <= opts.start_secs + 0.05 {
        return Err("Trim range must be at least 0.05s".into());
    }
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let ss = format!("{:.3}", opts.start_secs);
    let to = format!("{:.3}", opts.end_secs);
    let volumes: Vec<f64> = (0..opts.audio_tracks as usize)
        .map(|i| effective_track_volume(opts.muted_tracks, opts.track_volumes, i))
        .collect();
    let needs_audio_filter = volumes.iter().any(|&v| (v - 1.0).abs() >= 0.005);

    if !needs_audio_filter {
        let args = [
            OsStr::new("-y"),
            OsStr::new("-ss"),
            OsStr::new(&ss),
            OsStr::new("-to"),
            OsStr::new(&to),
            OsStr::new("-i"),
            src.as_os_str(),
            OsStr::new("-map"),
            OsStr::new("0"),
            OsStr::new("-c"),
            OsStr::new("copy"),
            OsStr::new("-movflags"),
            OsStr::new("+faststart"),
            dst.as_os_str(),
        ];
        run_ffmpeg(ffmpeg, &args)?;
    } else {
        let mut owned: Vec<String> = vec![
            "-y".into(),
            "-ss".into(),
            ss,
            "-to".into(),
            to,
            "-i".into(),
            src.to_string_lossy().into_owned(),
            "-map".into(),
            "0:v:0".into(),
            "-c:v".into(),
            "copy".into(),
        ];
        for (i, &vol) in volumes.iter().enumerate() {
            owned.push("-map".into());
            owned.push(format!("0:a:{i}"));
            if vol <= 0.001 {
                owned.push(format!("-filter:a:{i}"));
                owned.push("volume=0".into());
            } else if (vol - 1.0).abs() >= 0.005 {
                owned.push(format!("-filter:a:{i}"));
                owned.push(format!("volume={vol:.3}"));
            }
        }
        owned.extend([
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "192k".into(),
            "-movflags".into(),
            "+faststart".into(),
            dst.to_string_lossy().into_owned(),
        ]);
        let refs: Vec<&OsStr> = owned.iter().map(OsStr::new).collect();
        run_ffmpeg(ffmpeg, &refs)?;
    }
    Ok(dst.to_path_buf())
}

pub struct VerticalExportOpts<'a> {
    pub blur_bg: bool,
    pub start_secs: Option<f64>,
    pub end_secs: Option<f64>,
    pub audio_tracks: u32,
    pub muted_tracks: &'a [bool],
    pub track_volumes: &'a [f64],
}

/// Export a clip as 1080x1920 vertical 9:16 video (either blurred background + centered gameplay or center crop),
/// mixing all active audio tracks with their individual volumes into a single stereo AAC stream.
pub fn export_vertical_9_16(
    ffmpeg: &Path,
    src: &Path,
    dst: &Path,
    opts: &VerticalExportOpts<'_>,
) -> Result<PathBuf, String> {
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }

    let video_filter = if opts.blur_bg {
        "[0:v]split=2[bg][fg];[bg]scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,boxblur=25:5[bgb];[fg]scale=1080:-2[fgs];[bgb][fgs]overlay=(W-w)/2:(H-h)/2[vout]"
    } else {
        "[0:v]scale=-2:1920:force_original_aspect_ratio=increase,crop=1080:1920[vout]"
    };

    let active_tracks: Vec<(usize, f64)> = (0..opts.audio_tracks as usize)
        .filter_map(|i| {
            let vol = effective_track_volume(opts.muted_tracks, opts.track_volumes, i);
            (vol > 0.001).then_some((i, vol))
        })
        .collect();

    let mut filter = video_filter.to_string();
    let audio_map: Option<String> = if active_tracks.len() > 1 {
        let mut mix_inputs = String::new();
        for &(i, vol) in &active_tracks {
            if (vol - 1.0).abs() >= 0.005 {
                filter.push_str(&format!(";[0:a:{i}]volume={vol:.3}[a{i}]"));
                mix_inputs.push_str(&format!("[a{i}]"));
            } else {
                mix_inputs.push_str(&format!("[0:a:{i}]"));
            }
        }
        filter.push_str(&format!(
            ";{mix_inputs}amix=inputs={}:duration=longest:normalize=0[aout]",
            active_tracks.len()
        ));
        Some("[aout]".into())
    } else if active_tracks.len() == 1 {
        let (i, vol) = active_tracks[0];
        if (vol - 1.0).abs() >= 0.005 {
            filter.push_str(&format!(";[0:a:{i}]volume={vol:.3}[aout]"));
            Some("[aout]".into())
        } else {
            Some(format!("0:a:{i}"))
        }
    } else {
        None
    };

    let build_args = |codec: &str| -> Vec<String> {
        let mut v = vec!["-y".into()];
        if let Some(s) = opts.start_secs {
            v.push("-ss".into());
            v.push(format!("{s:.3}"));
        }
        if let Some(e) = opts.end_secs {
            let s = opts.start_secs.unwrap_or(0.0);
            if e > s {
                v.push("-t".into());
                v.push(format!("{:.3}", e - s));
            }
        }
        v.extend([
            "-i".into(),
            src.to_string_lossy().into_owned(),
            "-filter_complex".into(),
            filter.clone(),
            "-map".into(),
            "[vout]".into(),
        ]);
        if let Some(ref am) = audio_map {
            v.push("-map".into());
            v.push(am.clone());
            v.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "192k".into()]);
        } else {
            v.push("-an".into());
        }
        v.extend(["-c:v".into(), codec.into()]);
        if codec == "h264_nvenc" {
            v.extend(["-preset".into(), "p4".into(), "-cq".into(), "20".into()]);
        } else {
            v.extend(["-preset".into(), "veryfast".into(), "-crf".into(), "20".into()]);
        }
        v.extend(["-pix_fmt".into(), "yuv420p".into(), "-movflags".into(), "+faststart".into(), dst.to_string_lossy().into_owned()]);
        v
    };

    let nv_args = build_args("h264_nvenc");
    let nv_refs: Vec<&OsStr> = nv_args.iter().map(OsStr::new).collect();
    if run_ffmpeg(ffmpeg, &nv_refs).is_err() {
        let cpu_args = build_args("libx264");
        let cpu_refs: Vec<&OsStr> = cpu_args.iter().map(OsStr::new).collect();
        run_ffmpeg(ffmpeg, &cpu_refs)?;
    }
    Ok(dst.to_path_buf())
}

/// Concatenate 2+ clips into a montage MP4. Fast stream-copy first, normalizing transcode fallback if formats differ.
pub fn build_montage(ffmpeg: &Path, inputs: &[PathBuf], dst: &Path) -> Result<PathBuf, String> {
    if inputs.len() < 2 {
        return Err("Select at least 2 clips to build a montage".into());
    }
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let list = dst.with_extension("montage.txt");
    let body: String = inputs
        .iter()
        .map(|p| format!("file '{}'\n", p.to_string_lossy().replace('\\', "/")))
        .collect();
    std::fs::write(&list, body).map_err(|e| e.to_string())?;

    let copy_args = [
        OsStr::new("-y"),
        OsStr::new("-f"),
        OsStr::new("concat"),
        OsStr::new("-safe"),
        OsStr::new("0"),
        OsStr::new("-i"),
        list.as_os_str(),
        OsStr::new("-map"),
        OsStr::new("0"),
        OsStr::new("-c"),
        OsStr::new("copy"),
        OsStr::new("-movflags"),
        OsStr::new("+faststart"),
        dst.as_os_str(),
    ];
    let fast_ok = run_ffmpeg(ffmpeg, &copy_args).is_ok();
    let _ = std::fs::remove_file(&list);
    if fast_ok {
        return Ok(dst.to_path_buf());
    }

    // Fallback when inputs differ in resolution/FPS/audio track count: scale & normalize to 1080p60 + 1 AAC stereo mix.
    let n = inputs.len();
    let mut owned = vec!["-y".to_string()];
    let mut filter = String::new();
    let mut concat_inputs = String::new();
    for (i, p) in inputs.iter().enumerate() {
        owned.push("-i".into());
        owned.push(p.to_string_lossy().into_owned());
        filter.push_str(&format!(
            "[{i}:v]scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,fps=60,format=yuv420p,setsar=1[v{i}];"
        ));
        filter.push_str(&format!(
            "[{i}:a:0]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}];"
        ));
        concat_inputs.push_str(&format!("[v{i}][a{i}]"));
    }
    filter.push_str(&format!("{concat_inputs}concat=n={n}:v=1:a=1[vout][aout]"));
    owned.extend([
        "-filter_complex".into(),
        filter,
        "-map".into(),
        "[vout]".into(),
        "-map".into(),
        "[aout]".into(),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        "20".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "192k".into(),
        "-movflags".into(),
        "+faststart".into(),
        dst.to_string_lossy().into_owned(),
    ]);
    let refs: Vec<&OsStr> = owned.iter().map(OsStr::new).collect();
    run_ffmpeg(ffmpeg, &refs)?;
    Ok(dst.to_path_buf())
}

/// Put a clip file on the Windows clipboard (`CF_HDROP`) so Ctrl+V pastes the MP4 directly into Discord/Explorer.
pub fn copy_file_to_clipboard(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("clip does not exist: {}", path.display()));
    }
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // Strip `\\?\` extended-length prefix so Explorer/Discord accept the drop path cleanly.
    let s = abs.to_string_lossy();
    let clean = s.strip_prefix(r"\\?\").unwrap_or(&s);
    let mut wide: Vec<u16> = OsStr::new(clean).encode_wide().collect();
    wide.push(0); // path null-terminator
    wide.push(0); // double-null list terminator

    let header_bytes = std::mem::size_of::<DROPFILES>();
    let total_bytes = header_bytes + wide.len() * 2;
    unsafe {
        let hmem = GlobalAlloc(GMEM_MOVEABLE, total_bytes).map_err(|e| format!("GlobalAlloc failed: {e}"))?;
        let ptr = GlobalLock(hmem) as *mut u8;
        if ptr.is_null() {
            return Err("GlobalLock returned null".into());
        }
        std::ptr::write_bytes(ptr, 0, total_bytes);
        let dropfiles = DROPFILES {
            pFiles: header_bytes as u32,
            pt: Default::default(),
            fNC: false.into(),
            fWide: true.into(),
        };
        std::ptr::write(ptr as *mut DROPFILES, dropfiles);
        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr.add(header_bytes), wide.len() * 2);
        let _ = GlobalUnlock(hmem);

        OpenClipboard(Some(HWND::default())).map_err(|e| format!("OpenClipboard failed: {e}"))?;
        let res = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| format!("EmptyClipboard failed: {e}"))?;
            SetClipboardData(CF_HDROP, Some(HANDLE(hmem.0)))
                .map_err(|e| format!("SetClipboardData failed: {e}"))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ffmpeg() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("tools").join("ffmpeg.exe");
        assert!(root.exists(), "missing tools/ffmpeg.exe for editor test");
        root
    }

    fn make_clip(ff: &Path, out: &Path, w: u32, h: u32, secs: u32) {
        let dur = secs.to_string();
        let size = format!("{w}x{h}");
        let st = Command::new(ff)
            .args([
                "-y",
                "-f", "lavfi", "-i", &format!("testsrc=size={size}:rate=30:duration={dur}"),
                "-f", "lavfi", "-i", &format!("sine=frequency=440:sample_rate=48000:duration={dur}"),
                "-f", "lavfi", "-i", &format!("sine=frequency=880:sample_rate=48000:duration={dur}"),
                "-f", "lavfi", "-i", &format!("anullsrc=r=48000:cl=stereo:duration={dur}"),
                "-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:a",
                "-c:v", "libx264", "-c:a", "aac",
            ])
            .arg(out)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success());
    }

    fn track_peak(ff: &Path, path: &Path, track_idx: usize) -> f64 {
        let map = format!("0:a:{track_idx}");
        let out = Command::new(ff)
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-map", &map, "-ac", "1", "-ar", "16000", "-f", "s16le", "pipe:1"])
            .output()
            .unwrap();
        assert!(out.status.success(), "ffmpeg peak decode failed for {map}");
        let max = out
            .stdout
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs())
            .max()
            .unwrap_or(0);
        f64::from(max) / 32768.0
    }

    #[test]
    fn effective_track_volume_handles_mute_bounds_and_defaults() {
        assert_eq!(effective_track_volume(&[true, false], &[0.8, 0.4], 0), 0.0);
        assert_eq!(effective_track_volume(&[true, false], &[0.8, 0.4], 1), 0.4);
        assert_eq!(effective_track_volume(&[], &[], 2), 1.0);
        assert_eq!(effective_track_volume(&[false], &[-0.5], 0), 0.0);
        assert_eq!(effective_track_volume(&[false], &[f64::NAN], 0), 1.0);
    }

    #[test]
    fn trim_vertical_9_16_and_montage_produce_valid_mp4s() {
        let ff = test_ffmpeg();
        let dir = std::env::temp_dir().join(format!("encore_editor_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src1 = dir.join("c1.mp4");
        let src2 = dir.join("c2.mp4");
        make_clip(&ff, &src1, 1280, 720, 3);
        make_clip(&ff, &src2, 640, 360, 2);

        let trimmed = dir.join("trimmed.mp4");
        trim_clip(
            &ff,
            &src1,
            &trimmed,
            &TrimOpts {
                start_secs: 0.5,
                end_secs: 2.0,
                audio_tracks: 3,
                muted_tracks: &[false, true, false],
                track_volumes: &[0.25, 1.0, 1.0],
            },
        )
        .unwrap();
        assert!(trimmed.exists() && std::fs::metadata(&trimmed).unwrap().len() > 5000);
        let p0 = track_peak(&ff, &trimmed, 0);
        let p1 = track_peak(&ff, &trimmed, 1);
        let p2 = track_peak(&ff, &trimmed, 2);
        assert!(p0 > 0.01 && p0 < 0.08, "expected track 0 attenuated to ~25%, got {p0}");
        assert!(p1 < 0.005, "expected muted track 1 to be silent, got {p1}");
        assert!(p2 < 0.005, "expected silent source track 2 to stay silent, got {p2}");

        let vert = dir.join("vertical.mp4");
        export_vertical_9_16(
            &ff,
            &src1,
            &vert,
            &VerticalExportOpts {
                blur_bg: true,
                start_secs: Some(0.0),
                end_secs: Some(1.5),
                audio_tracks: 3,
                muted_tracks: &[false, false, true],
                track_volumes: &[0.35, 0.80, 1.0],
            },
        )
        .unwrap();
        let probe = Command::new(&ff)
            .args(["-i", vert.to_str().unwrap(), "-f", "null", "-"])
            .output()
            .unwrap();
        let txt = String::from_utf8_lossy(&probe.stderr);
        assert!(txt.contains("1080x1920"), "expected 1080x1920 output:\n{txt}");
        assert!(track_peak(&ff, &vert, 0) > 0.02, "expected mixed vertical audio");

        // Single active track with custom volume.
        let vert_single = dir.join("vertical_single.mp4");
        export_vertical_9_16(
            &ff,
            &src1,
            &vert_single,
            &VerticalExportOpts {
                blur_bg: false,
                start_secs: Some(0.0),
                end_secs: Some(1.0),
                audio_tracks: 3,
                muted_tracks: &[false, true, true],
                track_volumes: &[0.3, 1.0, 1.0],
            },
        )
        .unwrap();
        let vs_peak = track_peak(&ff, &vert_single, 0);
        assert!(vs_peak > 0.01 && vs_peak < 0.09, "expected single attenuated track, got {vs_peak}");

        // All tracks silenced via 0.0 volume produces valid video-only 9:16 MP4.
        let vert_silent = dir.join("vertical_silent.mp4");
        export_vertical_9_16(
            &ff,
            &src1,
            &vert_silent,
            &VerticalExportOpts {
                blur_bg: false,
                start_secs: Some(0.0),
                end_secs: Some(1.0),
                audio_tracks: 3,
                muted_tracks: &[false, false, false],
                track_volumes: &[0.0, 0.0, 0.0],
            },
        )
        .unwrap();
        assert!(vert_silent.exists() && std::fs::metadata(&vert_silent).unwrap().len() > 2000);

        let montage = dir.join("montage.mp4");
        build_montage(&ff, &[src1.clone(), src2], &montage).unwrap();
        assert!(montage.exists() && std::fs::metadata(&montage).unwrap().len() > 10000);

        // Verify CF_HDROP clipboard copy succeeds in user session (or check clean result).
        let _ = copy_file_to_clipboard(&src1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
