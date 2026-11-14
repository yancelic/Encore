//! Clip trimming, 9:16 vertical export, montage concatenation, and native clipboard sharing.
use std::ffi::OsStr;
use encore_common::platform::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(windows)]
#[path = "clipboard/windows.rs"]
mod clipboard;
#[cfg(not(windows))]
#[path = "clipboard/portable.rs"]
mod clipboard;
pub use clipboard::copy_file_to_clipboard;

fn run_ffmpeg(ffmpeg: &Path, args: &[&OsStr]) -> Result<(), String> {
    let out = Command::new(ffmpeg)
        .background()
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

/// Render a shareable MP4 with accurate trim boundaries and one stereo audio mix.
/// Source/editing clips keep their independent tracks; players used for sharing
/// generally play only the first audio stream, so they need a flattened mix.
pub fn export_share_clip(
    ffmpeg: &Path,
    src: &Path,
    dst: &Path,
    opts: &TrimOpts<'_>,
) -> Result<PathBuf, String> {
    if !opts.start_secs.is_finite() || !opts.end_secs.is_finite()
        || opts.start_secs < 0.0 || opts.end_secs <= opts.start_secs + 0.05
    {
        return Err("Share range must be finite, non-negative and at least 0.05s".into());
    }
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }

    let mut filter = String::new();
    let mut inputs = String::new();
    for i in 0..opts.audio_tracks as usize {
        let vol = effective_track_volume(opts.muted_tracks, opts.track_volumes, i);
        filter.push_str(&format!(
            "[0:a:{i}]volume={vol:.6},aformat=sample_rates=48000:channel_layouts=stereo[a{i}];"
        ));
        inputs.push_str(&format!("[a{i}]"));
    }
    if opts.audio_tracks > 0 {
        filter.push_str(&format!(
            "{inputs}amix=inputs={}:duration=longest:normalize=0[aout]", opts.audio_tracks
        ));
    }

    let build_args = |codec: &str| {
        let mut args: Vec<String> = vec![
            "-y".into(), "-ss".into(), format!("{:.6}", opts.start_secs),
            "-i".into(), src.to_string_lossy().into_owned(),
            "-t".into(), format!("{:.6}", opts.end_secs - opts.start_secs),
            "-map".into(), "0:v:0".into(),
        ];
        if opts.audio_tracks > 0 {
            args.extend([
                "-filter_complex".into(), filter.clone(), "-map".into(), "[aout]".into(),
                "-c:a".into(), "aac".into(), "-b:a".into(), "192k".into(),
                "-ac".into(), "2".into(), "-disposition:a:0".into(), "default".into(),
            ]);
        } else {
            args.push("-an".into());
        }
        // Decoding/re-encoding prevents stream copy from keeping frames before
        // the selection when the trim start falls between source keyframes.
        args.extend(["-c:v".into(), codec.into()]);
        if codec == "h264_nvenc" {
            args.extend(["-preset".into(), "p4".into(), "-cq".into(), "20".into()]);
        } else {
            args.extend(["-preset".into(), "veryfast".into(), "-crf".into(), "20".into()]);
        }
        args.extend([
            "-pix_fmt".into(), "yuv420p".into(), "-movflags".into(), "+faststart".into(),
            dst.to_string_lossy().into_owned(),
        ]);
        args
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
    /// Normalized [left, top, width, height]; None keeps the 9:16 presets.
    pub crop: Option<[f64; 4]>,
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

    let video_filter = if let Some([x, y, w, h]) = opts.crop {
        if ![x, y, w, h].iter().all(|v| v.is_finite()) || x < 0.0 || y < 0.0 || w <= 0.0 || h <= 0.0 || x + w > 1.000001 || y + h > 1.000001 {
            return Err("Crop must be a finite rectangle inside the source video".into());
        }
        format!("[0:v]crop=w='max(2,trunc(iw*{w:.8}/2)*2)':h='max(2,trunc(ih*{h:.8}/2)*2)':x='trunc(iw*{x:.8}/2)*2':y='trunc(ih*{y:.8}/2)*2',setsar=1[vout]")
    } else if opts.blur_bg {
        "[0:v]split=2[bg][fg];[bg]scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,boxblur=25:5[bgb];[fg]scale=1080:-2[fgs];[bgb][fgs]overlay=(W-w)/2:(H-h)/2[vout]".to_string()
    } else {
        "[0:v]scale=-2:1920:force_original_aspect_ratio=increase,crop=1080:1920[vout]".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ffmpeg() -> PathBuf {
        let ff = encore_common::ffmpeg_path();
        assert!(Command::new(&ff).arg("-version").output().is_ok(), "FFmpeg is required for media tests");
        ff
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
    fn share_renders_exact_range_and_all_three_sources_in_one_audio_stream() {
        let ff = test_ffmpeg();
        let dir = std::env::temp_dir().join(format!("encore_share_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("source.mp4");
        let output = Command::new(&ff).args([
            "-y", "-f", "lavfi", "-i",
            "color=c=red:s=320x180:r=30:d=3,drawbox=color=blue:t=fill:enable='gte(t,1)'",
            "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=3",
            "-f", "lavfi", "-i", "sine=frequency=880:sample_rate=48000:duration=3",
            "-f", "lavfi", "-i", "sine=frequency=1320:sample_rate=48000:duration=3",
            "-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:a",
            "-c:v", "libx264", "-g", "300", "-sc_threshold", "0", "-c:a", "aac",
        ]).arg(&src).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let original = std::fs::read(&src).unwrap();

        let opts = TrimOpts {
            start_secs: 1.25, end_secs: 2.05, audio_tracks: 3,
            muted_tracks: &[], track_volumes: &[],
        };
        let mixed = dir.join("mixed.mp4");
        export_share_clip(&ff, &src, &mixed, &opts).unwrap();

        let decode_audio = |path: &Path| {
            let out = Command::new(&ff).args(["-v", "error", "-i"]).arg(path)
                .args(["-map", "0:a:0", "-ac", "1", "-ar", "48000", "-f", "f32le", "pipe:1"])
                .output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            out.stdout.chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()) as f64).collect::<Vec<_>>()
        };
        let tone_amplitude = |samples: &[f64], hz: f64| {
            // Ignore AAC boundary padding; detect each source independently.
            let samples = &samples[4800..samples.len() - 4800];
            let (mut re, mut im) = (0.0, 0.0);
            for (i, sample) in samples.iter().enumerate() {
                let phase = std::f64::consts::TAU * hz * i as f64 / 48000.0;
                re += sample * phase.cos();
                im += sample * phase.sin();
            }
            2.0 * re.hypot(im) / samples.len() as f64
        };
        let samples = decode_audio(&mixed);
        assert!((samples.len() as f64 / 48000.0 - 0.8).abs() < 0.05);
        for hz in [440.0, 880.0, 1320.0] {
            assert!(tone_amplitude(&samples, hz) > 0.06, "missing source tone {hz}");
        }
        let info = Command::new(&ff).args(["-hide_banner", "-i"]).arg(&mixed).output().unwrap();
        let info = String::from_utf8_lossy(&info.stderr);
        assert_eq!(info.matches("Audio:").count(), 1, "{info}");
        assert!(info.contains("stereo"), "{info}");

        // The trim starts after the red section, between the source's keyframes.
        let frame = Command::new(&ff).args(["-v", "error", "-i"]).arg(&mixed)
            .args(["-map", "0:v:0", "-frames:v", "1", "-pix_fmt", "rgb24", "-f", "rawvideo", "pipe:1"])
            .output().unwrap();
        assert!(frame.status.success());
        assert!(frame.stdout[2] > 200 && frame.stdout[0] < 30, "trim retained pre-selection frames");

        let levels = dir.join("levels.mp4");
        export_share_clip(&ff, &src, &levels, &TrimOpts {
            muted_tracks: &[true, false, false], track_volumes: &[1.0, 0.25, 0.5], ..opts
        }).unwrap();
        let levels = decode_audio(&levels);
        assert!(tone_amplitude(&levels, 440.0) < 0.003, "game mute ignored");
        for (hz, gain) in [(880.0, 0.25), (1320.0, 0.5)] {
            let ratio = tone_amplitude(&levels, hz) / tone_amplitude(&samples, hz);
            assert!((ratio - gain).abs() < 0.04, "volume ignored for {hz}: {ratio}");
        }

        let silent = dir.join("silent.mp4");
        export_share_clip(&ff, &src, &silent, &TrimOpts {
            muted_tracks: &[true, true, true], ..opts
        }).unwrap();
        assert!(track_peak(&ff, &silent, 0) < 0.001);

        let no_audio = dir.join("no_audio.mp4");
        let video = Command::new(&ff).args(["-y", "-i"]).arg(&src)
            .args(["-map", "0:v:0", "-c", "copy", "-an"]).arg(&no_audio).output().unwrap();
        assert!(video.status.success());
        let video_share = dir.join("video_share.mp4");
        export_share_clip(&ff, &no_audio, &video_share, &TrimOpts { audio_tracks: 0, ..opts }).unwrap();
        let probe = Command::new(&ff).args(["-hide_banner", "-i"]).arg(&video_share).output().unwrap();
        let probe = String::from_utf8_lossy(&probe.stderr);
        assert!(probe.contains("Video:") && !probe.contains("Audio:"), "{probe}");

        for (start, end) in [(f64::NAN, 2.0), (0.0, f64::INFINITY), (-1.0, 2.0), (2.0, 1.0)] {
            assert!(export_share_clip(&ff, &src, &mixed, &TrimOpts {
                start_secs: start, end_secs: end, ..opts
            }).is_err());
        }
        assert_eq!(std::fs::read(&src).unwrap(), original, "source clip was modified");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn custom_crop_exports_selected_dimensions_and_rejects_invalid_bounds() {
        let ff = test_ffmpeg();
        let dir = std::env::temp_dir().join(format!("encore_crop_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("source.mp4");
        let dst = dir.join("crop.mp4");
        make_clip(&ff, &src, 640, 360, 1);
        let mut opts = VerticalExportOpts { crop: Some([0.25, 0.0, 0.5, 1.0]), blur_bg: false, start_secs: Some(0.0), end_secs: Some(0.5), audio_tracks: 3, muted_tracks: &[false, true, true], track_volumes: &[0.5, 1.0, 1.0] };
        export_vertical_9_16(&ff, &src, &dst, &opts).unwrap();
        let probe = Command::new(&ff).args(["-i", dst.to_str().unwrap(), "-f", "null", "-"]).output().unwrap();
        let info = String::from_utf8_lossy(&probe.stderr);
        assert!(probe.status.success() && info.contains("320x360"), "{info}");
        for invalid in [[0.9, 0.0, 0.5, 1.0], [0.0, 0.0, -0.5, 1.0], [f64::NAN, 0.0, 0.5, 1.0]] {
            opts.crop = Some(invalid);
            assert!(export_vertical_9_16(&ff, &src, &dir.join("invalid.mp4"), &opts).is_err());
        }
        let _ = std::fs::remove_dir_all(dir);
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
                crop: None,
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
                crop: None,
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
                crop: None,
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
        // Clipboard integration is verified manually in a native desktop session.

        let _ = std::fs::remove_dir_all(&dir);
    }
}
