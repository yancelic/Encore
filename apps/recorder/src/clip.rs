//! Explicit RAM replay export: compressed MP4 muxing and metadata/faststart stream copy.
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub use encore_common::ffmpeg_path;

/// Mux a frozen, shared packet snapshot without touching the live encoder.
/// Only this explicit save path creates files; FFmpeg adds metadata/faststart by stream copy.
pub fn save_replay(
    snapshot: crate::encoder::ReplaySnapshot,
    out: &Path,
    game_name: Option<&str>,
) -> Result<(), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Media::MediaFoundation::*;
    crate::encoder::ensure_com_mta();
    let parent = out.parent().ok_or("clip output has no parent directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = out.file_stem().unwrap_or_default().to_string_lossy();
    let raw = parent.join(format!(".tmp_{name}_raw.mp4"));
    let ready = parent.join(format!(".tmp_{name}_ready.mp4"));
    let mux = || -> Result<(), crate::encoder::Err> {
        unsafe {
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 1)?;
            let attrs = attrs.unwrap();
            attrs.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
            let url = HSTRING::from(raw.as_os_str());
            let writer = MFCreateSinkWriterFromURL(PCWSTR(url.as_ptr()), None, &attrs)?;
            let mut ids = Vec::new();
            for format in &snapshot.formats {
                let id = writer.AddStream(format)?;
                writer.SetInputMediaType(id, format, None)?; // compressed -> identical compressed: no encoder
                ids.push(id);
            }
            writer.BeginWriting()?;
            let base = snapshot
                .packets
                .streams
                .iter()
                .flatten()
                .map(|p| p.info.pts.min(p.info.dts))
                .min()
                .ok_or("empty replay snapshot")?;
            let mut positions = [0usize; encore_encode::STREAMS];
            loop {
                let next = (0..encore_encode::STREAMS)
                    .filter_map(|s| snapshot.packets.streams[s].get(positions[s]).map(|p| (s, p)))
                    .min_by_key(|(_, p)| p.info.dts);
                let Some((stream, packet)) = next else { break };
                let bytes = packet.data.bytes();
                let buffer = MFCreateMemoryBuffer(bytes.len().try_into()?)?;
                let mut address = std::ptr::null_mut();
                buffer.Lock(&mut address, None, None)?;
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), address, bytes.len());
                buffer.Unlock()?;
                buffer.SetCurrentLength(bytes.len() as u32)?;
                let sample = MFCreateSample()?;
                sample.AddBuffer(&buffer)?;
                sample.SetSampleTime(packet.info.pts - base)?;
                sample.SetSampleDuration(packet.info.duration)?;
                sample.SetUINT64(&MFSampleExtension_DecodeTimestamp, (packet.info.dts - base) as u64)?;
                if packet.info.keyframe {
                    sample.SetUINT32(&MFSampleExtension_CleanPoint, 1)?;
                }
                writer.WriteSample(ids[stream], &sample)?;
                positions[stream] += 1;
            }
            writer.Finalize()?;
        }
        Ok(())
    };
    let mux_result = mux().map_err(|e| format!("replay mux failed: {e}"));
    // The raw MP4 now owns the data. Release old packet references before the disk-only
    // metadata/faststart pass, so a slow disk does not unnecessarily starve live history.
    drop(snapshot);
    let result = mux_result.and_then(|()| {
        let mut command = Command::new(ffmpeg_path());
        command.creation_flags(CREATE_NO_WINDOW).args(["-y", "-v", "error", "-i"]).arg(&raw).args([
            "-map",
            "0",
            "-c",
            "copy",
            "-movflags",
            "+faststart",
        ]);
        if let Some(game) = game_name {
            command.args(["-metadata", &format!("comment=encore_game:{game}")]);
        }
        let result = command
            .arg(&ready)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("starting replay FFmpeg: {e}"))?;
        if !result.status.success() {
            return Err(format!("replay metadata mux failed: {}", String::from_utf8_lossy(&result.stderr)));
        }
        if out.exists() {
            return Err("clip output already exists".into());
        }
        std::fs::rename(&ready, out).map_err(|e| format!("publishing replay MP4: {e}"))
    });
    let _ = std::fs::remove_file(raw);
    let _ = std::fs::remove_file(ready);
    result
}
