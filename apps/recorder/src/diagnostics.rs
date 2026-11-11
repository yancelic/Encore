//! Opt-in recorder diagnostics. These measure capture/submission work, not input-to-display latency.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Default, Clone, Copy)]
pub struct CaptureCounters {
    pub received: u64,
    pub drained: u64,
    pub throttled: u64,
    pub busy_drops: u64,
    pub copied: u64,
    pub superseded: u64,
    pub copy_callback_ns: u64,
    pub copy_callback_max_ns: u64,
}

#[derive(Default)]
pub struct ReplayCounters {
    pub bytes: usize,
    pub limit: usize,
    pub history_ms: u128,
    pub memory_drops: u64,
    pub memory_evictions: u64,
    pub pending_saves: usize,
}

#[derive(Default)]
struct Timing {
    count: u64,
    total_ns: u128,
    max_ns: u128,
}

impl Timing {
    fn push(&mut self, ns: u128) {
        self.count += 1;
        self.total_ns += ns;
        self.max_ns = self.max_ns.max(ns);
    }

    fn avg_ms(&self) -> f64 {
        self.total_ns as f64 / self.count.max(1) as f64 / 1_000_000.0
    }
}

pub struct Stats {
    since: Instant,
    file: Option<File>,
    encoded: u64,
    repeated: u64,
    video_errors: u64,
    audio_errors: u64,
    late_ticks: u64,
    submission: Timing,
    frame_age: Timing,
}

impl Stats {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("ENCORE_STATS_FILE");
        if path.is_none() && std::env::var_os("ENCORE_STATS").is_none() {
            return None;
        }
        let file = path.and_then(|p| match OpenOptions::new().create(true).append(true).open(p) {
            Ok(file) => Some(file),
            Err(e) => {
                eprintln!("Could not open ENCORE_STATS_FILE: {e}");
                None
            }
        });
        Some(Self::new(file))
    }

    fn new(file: Option<File>) -> Self {
        Self {
            since: Instant::now(),
            file,
            encoded: 0,
            repeated: 0,
            video_errors: 0,
            audio_errors: 0,
            late_ticks: 0,
            submission: Timing::default(),
            frame_age: Timing::default(),
        }
    }

    pub fn video(&mut self, ok: bool, repeated: bool, submission_ns: u128, frame_age_100ns: i64) {
        self.submission.push(submission_ns);
        if ok {
            self.encoded += 1;
            self.repeated += u64::from(repeated);
            self.frame_age.push(frame_age_100ns.max(0) as u128 * 100);
        } else {
            self.video_errors += 1;
        }
    }

    pub fn audio_error(&mut self) {
        self.audio_errors += 1;
    }
    pub fn late_tick(&mut self) {
        self.late_ticks += 1;
    }
    pub fn due(&self) -> bool {
        self.since.elapsed().as_secs_f64() >= 5.0
    }

    pub fn report(
        &mut self,
        capture: CaptureCounters,
        slots: usize,
        input: (u32, u32),
        output: (u32, u32),
        fps: u32,
        replay: ReplayCounters,
    ) {
        let seconds = self.since.elapsed().as_secs_f64().max(0.001);
        // Copy-callback time includes CPU-side work/command submission, not GPU completion.
        let callback_avg = capture.copy_callback_ns as f64 / capture.copied.max(1) as f64 / 1_000_000.0;
        let gpu_bytes = input.0 as u64 * input.1 as u64 * 4 * slots as u64;
        let record = serde_json::json!({
            "schema": 1,
            "unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
            "interval_secs": seconds, "target_fps": fps, "input_size": [input.0, input.1],
            "output_size": [output.0, output.1],
            "received_frames": capture.received, "drained_frames": capture.drained,
            "throttled_frames": capture.throttled, "busy_drops": capture.busy_drops,
            "copied_frames": capture.copied, "superseded_frames": capture.superseded,
            "submitted_frames": self.encoded, "repeated_samples": self.repeated,
            "copied_fps": capture.copied as f64 / seconds, "submitted_fps": self.encoded as f64 / seconds,
            "copy_callback_avg_ms": callback_avg, "copy_callback_max_ms": capture.copy_callback_max_ns as f64 / 1_000_000.0,
            "video_submit_avg_ms": self.submission.avg_ms(), "video_submit_max_ms": self.submission.max_ns as f64 / 1_000_000.0,
            "frame_age_avg_ms": self.frame_age.avg_ms(), "frame_age_max_ms": self.frame_age.max_ns as f64 / 1_000_000.0,
            "late_ticks": self.late_ticks, "video_errors": self.video_errors, "audio_errors": self.audio_errors,
            "gpu_slots": slots, "gpu_slot_bytes": gpu_bytes,
            "replay_bytes": replay.bytes, "replay_limit_bytes": replay.limit,
            "replay_history_ms": replay.history_ms as u64, "replay_memory_drops": replay.memory_drops,
            "replay_memory_evictions": replay.memory_evictions, "pending_saves": replay.pending_saves,
        });
        println!("[stats] {record}");
        if let Some(file) = self.file.as_mut() {
            if let Err(e) = writeln!(file, "{record}") {
                eprintln!("Writing recorder stats failed: {e}");
                self.file = None;
            }
        }
        self.since = Instant::now();
        self.encoded = 0;
        self.repeated = 0;
        self.video_errors = 0;
        self.audio_errors = 0;
        self.late_ticks = 0;
        self.submission = Timing::default();
        self.frame_age = Timing::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_uses_all_samples_and_preserves_spikes() {
        let mut timing = Timing::default();
        timing.push(1_000_000);
        timing.push(9_000_000);
        assert_eq!(timing.avg_ms(), 5.0);
        assert_eq!(timing.max_ns, 9_000_000);
        assert_eq!(Timing::default().avg_ms(), 0.0);
    }

    #[test]
    fn report_writes_jsonl_and_resets_interval_counters() {
        let path = std::env::temp_dir().join(format!("encore_stats_test_{}.jsonl", std::process::id()));
        let mut stats = Stats::new(Some(File::create(&path).unwrap()));
        stats.video(true, false, 1_000_000, 20_000);
        stats.video(true, true, 3_000_000, 40_000);
        stats.video(false, false, 2_000_000, 0);
        stats.audio_error();
        stats.late_tick();
        stats.report(
            CaptureCounters { received: 120, copied: 60, throttled: 60, ..Default::default() },
            6,
            (640, 360),
            (640, 360),
            60,
            ReplayCounters { bytes: 100, limit: 1000, ..Default::default() },
        );
        let row: serde_json::Value = serde_json::from_str(std::fs::read_to_string(&path).unwrap().trim()).unwrap();
        assert_eq!(row["submitted_frames"], 2);
        assert_eq!(row["repeated_samples"], 1);
        assert_eq!(row["video_errors"], 1);
        assert_eq!(row["audio_errors"], 1);
        assert_eq!(row["frame_age_avg_ms"], 3.0);
        assert_eq!(row["gpu_slot_bytes"], 640 * 360 * 4 * 6);
        assert_eq!(row["replay_bytes"], 100);
        assert_eq!(row["replay_limit_bytes"], 1000);
        assert_eq!(stats.encoded, 0);
        assert_eq!(stats.submission.count, 0);
        drop(stats);
        std::fs::remove_file(path).unwrap();
    }
}
