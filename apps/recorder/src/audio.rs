//! Multi-track WASAPI capture (loopback for game/chat, input for microphone) + QPC timeline.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wasapi::{AudioClient, Device, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};
use encore_common::{ipc::RecorderStatus, processes};

use crate::clock;

pub const SAMPLE_RATE: i64 = 48_000;
pub const BYTES_PER_FRAME: usize = 4; // 2 channels * 2 bytes (S16)
const BUFFER_DURATION_HNS: i64 = 2_000_000; // 200 ms shared buffer prevents packet drops
const CONTIGUOUS_TOLERANCE_SAMPLES: i64 = 720; // +/- 15 ms: stay sample-contiguous without clicks

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSourceKind {
    Loopback,
    Capture,
}

pub enum AudioSource {
    Device(AudioSourceKind, Option<String>),
    Process(u32),
    Application(String),
}

pub fn warning(status: &Mutex<RecorderStatus>, track: usize, message: Option<String>) {
    let mut status = status.lock().unwrap();
    if status.audio_warnings[track] != message {
        if let Some(ref message) = message { eprintln!("audio: {message}"); }
        status.audio_warnings[track] = message;
    }
}

/// Raw captured PCM chunk tagged with the QPC timestamp (100-ns units) of its first sample.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub start_100ns: i64,
    pub pcm: Vec<u8>,
}

fn find_device(en: &DeviceEnumerator, dir: Direction, want: Option<&str>) -> Result<Device, Box<dyn std::error::Error>> {
    match want {
        Some(w) if !w.eq_ignore_ascii_case("default") && !w.is_empty() => {
            let coll = en.get_device_collection(dir)?;
            for i in 0..coll.get_nbr_devices()? {
                let d = coll.get_device_at_index(i)?;
                if d.get_friendlyname()?.to_lowercase().contains(&w.to_lowercase()) {
                    return Ok(d);
                }
            }
            Err(format!("no {:?} device matches '{w}'", dir).into())
        }
        _ => Ok(en.get_default_device(dir)?),
    }
}

/// Upconvert 16-bit mono PCM samples into interleaved stereo S16.
fn mono_to_stereo(mono: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(mono.len() * 2);
    for sample in mono.chunks_exact(2) {
        out.extend_from_slice(sample);
        out.extend_from_slice(sample);
    }
    out
}

/// Runs until `stop` is set, sending 48 kHz stereo S16 chunks tagged with QPC timestamps.
pub fn run(
    kind: AudioSourceKind,
    label: &'static str,
    tx: SyncSender<AudioChunk>,
    want: Option<String>,
    stop: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    if want.as_deref().is_some_and(|w| w.eq_ignore_ascii_case("off")) {
        return Ok(());
    }
    wasapi::initialize_mta().ok()?;
    let en = DeviceEnumerator::new()?;
    let enum_dir = match kind {
        AudioSourceKind::Loopback => Direction::Render,
        AudioSourceKind::Capture => Direction::Capture,
    };
    let device = find_device(&en, enum_dir, want.as_deref())?;
    println!("audio [{label}]: {}", device.get_friendlyname()?);

    let mut client = device.get_iaudioclient()?;
    let mut channels = 2usize;
    let fmt_stereo = WaveFormat::new(16, 16, SampleType::Int, SAMPLE_RATE as usize, 2, None);
    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: BUFFER_DURATION_HNS };
    if client.initialize_client(&fmt_stereo, Direction::Capture, mode, None).is_err() {
        // Some microphones only accept mono even in shared mode; retry 1-ch and upmix.
        client = device.get_iaudioclient()?;
        let fmt_mono = WaveFormat::new(16, 16, SampleType::Int, SAMPLE_RATE as usize, 1, None);
        let mode2 = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: BUFFER_DURATION_HNS };
        client.initialize_client(&fmt_mono, Direction::Capture, mode2, None)?;
        channels = 1;
    }

    capture_stream(&client, channels, &tx, &stop, || false).map(|_| ())
}

/// One bounded capture queue per track. If the encoder stalls, discard packets
/// instead of blocking WASAPI or allowing raw audio memory to grow indefinitely.
fn capture_stream(
    client: &AudioClient,
    channels: usize,
    tx: &SyncSender<AudioChunk>,
    stop: &AtomicBool,
    mut reconnect: impl FnMut() -> bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let event = client.set_get_eventhandle()?;
    let cap = client.get_audiocaptureclient()?;
    client.start_stream()?;
    let result = (|| {
        let mut queue = VecDeque::new();
        while !stop.load(Ordering::Relaxed) {
            if reconnect() { return Ok(true); }
            while !stop.load(Ordering::Relaxed) && cap.get_next_packet_size()?.unwrap_or(0) > 0 {
                let info = cap.read_from_device_to_deque(&mut queue)?;
                if queue.is_empty() { continue; }
                let raw: Vec<u8> = queue.drain(..).collect();
                let pcm = if channels == 1 { mono_to_stereo(&raw) } else { raw };
                let frames = (pcm.len() / BYTES_PER_FRAME) as i64;
                let now = clock::qpc_100ns();
                let dur_100ns = (frames * 10_000_000) / SAMPLE_RATE;
                let start_100ns = if info.timestamp > 0 && (now - info.timestamp as i64).abs() < 5_000_000 {
                    info.timestamp as i64
                } else { now - dur_100ns };
                if let Err(TrySendError::Disconnected(_)) = tx.try_send(AudioChunk { start_100ns, pcm }) {
                    return Ok(false);
                }
            }
            let _ = event.wait_for_event(20);
        }
        Ok(false)
    })();
    let _ = client.stop_stream();
    result
}

/// Windows application loopback includes descendants without injecting into them.
/// A chat application's executable is resolved again every second; titles never
/// participate. Do not broaden to whole-device recording on failures.
pub fn run_application(
    source: AudioSource, track: usize, tx: SyncSender<AudioChunk>, stop: Arc<AtomicBool>,
    status: Arc<Mutex<RecorderStatus>>,
) {
    if !processes::application_loopback_supported() {
        warning(&status, track, Some("Application audio requires Windows build 20348 or later. Select Devices to route audio manually.".into()));
        return;
    }
    if let Err(error) = wasapi::initialize_mta().ok() {
        warning(&status, track, Some(format!("Application audio could not initialize: {error}")));
        return;
    }
    let mut preferred = None;
    while !stop.load(Ordering::Relaxed) {
        let pid = match &source {
            AudioSource::Process(pid) => Some(*pid),
            AudioSource::Application(exe) => processes::application_root(&processes::snapshot(), exe, preferred),
            AudioSource::Device(..) => unreachable!(),
        };
        let Some(pid) = pid else {
            if let AudioSource::Application(exe) = &source {
                warning(&status, track, Some(format!("{exe} is not running. Chat audio will reconnect when it opens.")));
            }
            wait_retry(&stop);
            continue;
        };
        preferred = Some(pid);
        let result = (|| -> Result<bool, Box<dyn std::error::Error>> {
            let client = application_client(pid)?;
            warning(&status, track, None);
            let mut check_at = Instant::now() + Duration::from_secs(1);
            capture_stream(&client, 2, &tx, &stop, || {
                let AudioSource::Application(exe) = &source else { return false };
                if Instant::now() < check_at { return false; }
                check_at = Instant::now() + Duration::from_secs(1);
                processes::application_root(&processes::snapshot(), exe, preferred) != Some(pid)
            })
        })();
        if matches!(result, Ok(false)) { break; }
        if let Err(error) = result {
            warning(&status, track, Some(format!("{} application audio is unavailable; retrying. Select Devices if this persists. ({error})", if track == 0 { "Game" } else { "Chat" })));
            wait_retry(&stop);
        }
    }
}

fn wait_retry(stop: &AtomicBool) {
    for _ in 0..20 {
        if stop.load(Ordering::Relaxed) { break; }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn application_client(pid: u32) -> Result<AudioClient, Box<dyn std::error::Error>> {
    let mut client = AudioClient::new_application_loopback_client(pid, true)?;
    let format = WaveFormat::new(16, 16, SampleType::Int, SAMPLE_RATE as usize, 2, None);
    client.initialize_client(&format, Direction::Capture,
        StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: BUFFER_DURATION_HNS }, None)?;
    Ok(client)
}

pub use encore_common::ipc::list_audio_devices as list_devices;

/// Diagnostic: probe every output (loopback) and input (mic) device for 4s and report bytes received.
pub fn diag() {
    wasapi::initialize_mta().ok().unwrap();
    let en = DeviceEnumerator::new().unwrap();
    for (title, enum_dir) in [("Output (Loopback)", Direction::Render), ("Input (Microphone)", Direction::Capture)] {
        let def = en
            .get_default_device(enum_dir)
            .ok()
            .and_then(|d| d.get_friendlyname().ok())
            .unwrap_or_else(|| "<none>".into());
        println!("=== {title} | default: {def} ===");
        let Ok(coll) = en.get_device_collection(enum_dir) else { continue };
        for i in 0..coll.get_nbr_devices().unwrap_or(0) {
            let Ok(dev) = coll.get_device_at_index(i) else { continue };
            let name = dev.get_friendlyname().unwrap_or_default();
            let Ok(mut client) = dev.get_iaudioclient() else { continue };
            let fmt = WaveFormat::new(16, 16, SampleType::Int, SAMPLE_RATE as usize, 2, None);
            let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: BUFFER_DURATION_HNS };
            if let Err(e) = client.initialize_client(&fmt, Direction::Capture, mode, None) {
                println!("  [{name}] init failed: {e}");
                continue;
            }
            let ev = client.set_get_eventhandle().unwrap();
            let cap = client.get_audiocaptureclient().unwrap();
            client.start_stream().unwrap();
            let mut q = VecDeque::new();
            let t = Instant::now();
            let mut bytes = 0usize;
            while t.elapsed() < Duration::from_secs(4) {
                while cap.get_next_packet_size().ok().flatten().unwrap_or(0) > 0 {
                    let _ = cap.read_from_device_to_deque(&mut q);
                    bytes += q.len();
                    q.clear();
                }
                let _ = ev.wait_for_event(20);
            }
            println!("  [{name}] {bytes} bytes");
        }
    }
}

/// Aligns timestamped audio chunks onto a gap-free sample timeline anchored at `t0_100ns`.
pub struct AudioTimeline {
    t0_100ns: i64,
    cursor_samples: i64,
    stream_sample_pos: Option<i64>,
    chunks: VecDeque<(i64, Vec<u8>)>,
}

impl AudioTimeline {
    pub fn new(t0_100ns: i64) -> Self {
        Self {
            t0_100ns,
            cursor_samples: 0,
            stream_sample_pos: None,
            chunks: VecDeque::new(),
        }
    }

    /// Push a captured chunk, placing it on the timeline relative to `t0_100ns`.
    pub fn push(&mut self, chunk: AudioChunk) {
        let frames = (chunk.pcm.len() / BYTES_PER_FRAME) as i64;
        if frames == 0 {
            return;
        }
        let rel_100ns = chunk.start_100ns - self.t0_100ns;
        let qpc_sample = ((rel_100ns as i128 * SAMPLE_RATE as i128) / 10_000_000) as i64;
        // Keep consecutive packets seamless within +/-15 ms to avoid micro-clicks from QPC jitter;
        // snap to the true QPC position after silence gaps or cumulative clock drift > 15 ms.
        let placed = match self.stream_sample_pos {
            Some(prev) if (qpc_sample - prev).abs() <= CONTIGUOUS_TOLERANCE_SAMPLES => prev,
            _ => qpc_sample,
        };
        let end = placed + frames;
        self.stream_sample_pos = Some(end);
        if end > self.cursor_samples {
            self.chunks.push_back((placed, chunk.pcm));
        }
    }

    /// Pop exactly `count` stereo S16 sample frames (`count * 4` bytes), filling any silence with zeros.
    pub fn pop_samples(&mut self, count: usize) -> Vec<u8> {
        let mut out = vec![0u8; count * BYTES_PER_FRAME];
        let win_start = self.cursor_samples;
        let win_end = win_start + count as i64;

        for (chunk_start, pcm) in &self.chunks {
            let chunk_frames = (pcm.len() / BYTES_PER_FRAME) as i64;
            let chunk_end = *chunk_start + chunk_frames;
            let ov_start = win_start.max(*chunk_start);
            let ov_end = win_end.min(chunk_end);
            if ov_start < ov_end {
                let src_off = ((ov_start - *chunk_start) as usize) * BYTES_PER_FRAME;
                let dst_off = ((ov_start - win_start) as usize) * BYTES_PER_FRAME;
                let len = ((ov_end - ov_start) as usize) * BYTES_PER_FRAME;
                out[dst_off..dst_off + len].copy_from_slice(&pcm[src_off..src_off + len]);
            }
        }

        while let Some((chunk_start, pcm)) = self.chunks.front() {
            let chunk_end = *chunk_start + (pcm.len() / BYTES_PER_FRAME) as i64;
            if chunk_end <= win_end {
                self.chunks.pop_front();
            } else {
                break;
            }
        }

        self.cursor_samples = win_end;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk_with_val(start_ms: i64, samples: usize, val: i16) -> AudioChunk {
        let mut pcm = Vec::with_capacity(samples * BYTES_PER_FRAME);
        for _ in 0..samples {
            pcm.extend_from_slice(&val.to_le_bytes());
            pcm.extend_from_slice(&val.to_le_bytes());
        }
        AudioChunk { start_100ns: start_ms * 10_000, pcm }
    }

    fn read_sample(buf: &[u8], idx: usize) -> i16 {
        let off = idx * BYTES_PER_FRAME;
        i16::from_le_bytes([buf[off], buf[off + 1]])
    }

    #[test]
    fn discards_preroll_and_trims_straddling_first_chunk() {
        let mut tl = AudioTimeline::new(100 * 10_000);
        tl.push(chunk_with_val(50, 480, 111));
        tl.push(chunk_with_val(95, 480, 222));
        let out = tl.pop_samples(480);
        assert_eq!(out.len(), 480 * BYTES_PER_FRAME);
        assert_eq!(read_sample(&out, 0), 222);
        assert_eq!(read_sample(&out, 239), 222);
        assert_eq!(read_sample(&out, 240), 0);
    }

    #[test]
    fn pads_short_silence_gap_without_deadband_drift() {
        let mut tl = AudioTimeline::new(0);
        tl.push(chunk_with_val(0, 480, 100));
        tl.push(chunk_with_val(40, 480, 300));
        let out = tl.pop_samples(2400);
        assert_eq!(read_sample(&out, 0), 100);
        assert_eq!(read_sample(&out, 479), 100);
        assert_eq!(read_sample(&out, 480), 0);
        assert_eq!(read_sample(&out, 1919), 0);
        assert_eq!(read_sample(&out, 1920), 300);
        assert_eq!(read_sample(&out, 2399), 300);
    }

    #[test]
    fn absorbs_small_qpc_jitter_without_gaps_and_clamps_large_drift() {
        let mut tl = AudioTimeline::new(0);
        tl.push(chunk_with_val(0, 480, 10));
        tl.push(chunk_with_val(11, 480, 20));
        tl.push(chunk_with_val(40, 480, 30));
        let out = tl.pop_samples(2400);
        assert_eq!(read_sample(&out, 479), 10);
        assert_eq!(read_sample(&out, 480), 20);
        assert_eq!(read_sample(&out, 959), 20);
        assert_eq!(read_sample(&out, 960), 0);
        assert_eq!(read_sample(&out, 1920), 30);
    }

    #[test]
    fn mono_to_stereo_duplicates_samples() {
        let mono = [100i16.to_le_bytes(), (-200i16).to_le_bytes()].concat();
        let stereo = mono_to_stereo(&mono);
        assert_eq!(stereo.len(), 8);
        assert_eq!(read_sample(&stereo, 0), 100);
        assert_eq!(i16::from_le_bytes([stereo[2], stereo[3]]), 100);
        assert_eq!(read_sample(&stereo, 1), -200);
        assert_eq!(i16::from_le_bytes([stereo[6], stereo[7]]), -200);
    }
}
