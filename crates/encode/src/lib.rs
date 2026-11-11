//! Bounded, multi-track replay history of already compressed packets.
//! Payload ownership is shared with saves; a pending save cannot escape the memory budget.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const STREAMS: usize = 4; // H.264 video, game AAC, chat AAC, microphone AAC
const MAX_PACKETS: usize = 131_072;

#[derive(Debug, Clone, Copy)]
pub struct PacketInfo {
    pub stream: usize,
    pub pts: i64,
    pub dts: i64,
    pub duration: i64,
    pub keyframe: bool,
}

#[derive(Debug)]
pub struct Payload {
    bytes: Box<[u8]>,
    live_bytes: Arc<AtomicUsize>,
}

impl Payload {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl Drop for Payload {
    fn drop(&mut self) {
        self.live_bytes.fetch_sub(self.bytes.len(), Ordering::Relaxed);
    }
}

#[derive(Debug, Clone)]
pub struct Packet {
    pub info: PacketInfo,
    pub data: Arc<Payload>,
}

#[derive(Debug)]
pub struct Snapshot {
    /// Each stream stays in decode order. Exporters must preserve PTS/DTS offsets.
    pub streams: [Vec<Packet>; STREAMS],
    pub start: i64,
    pub end: i64,
}

impl Snapshot {
    pub fn duration(&self) -> Duration {
        Duration::from_nanos(self.end.saturating_sub(self.start).max(0) as u64 * 100)
    }
}

#[derive(Clone, Copy)]
struct Keyframe {
    pts: i64,
    dts: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushResult {
    Stored,
    WaitingForKeyframe,
    MemoryLimited,
    Invalid,
}

pub struct RingBuffer {
    window: i64,
    max_bytes: usize,
    live_bytes: Arc<AtomicUsize>,
    streams: [VecDeque<Packet>; STREAMS],
    keys: VecDeque<Keyframe>,
    ends: [Option<i64>; STREAMS],
    count: usize,
    memory_drops: u64,
    memory_evictions: u64,
}

impl RingBuffer {
    pub fn new(duration: Duration, max_bytes: usize) -> Self {
        Self {
            window: (duration.as_nanos() / 100).min(i64::MAX as u128) as i64,
            max_bytes,
            live_bytes: Arc::new(AtomicUsize::new(0)),
            streams: std::array::from_fn(|_| VecDeque::new()),
            keys: VecDeque::new(),
            ends: [None; STREAMS],
            count: 0,
            memory_drops: 0,
            memory_evictions: 0,
        }
    }

    pub fn live_bytes(&self) -> usize {
        self.live_bytes.load(Ordering::Relaxed)
    }
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }
    pub fn memory_drops(&self) -> u64 {
        self.memory_drops
    }
    pub fn memory_evictions(&self) -> u64 {
        self.memory_evictions
    }
    pub fn available_duration(&self) -> Duration {
        let end = self.ends.iter().copied().collect::<Option<Vec<_>>>().and_then(|v| v.into_iter().min());
        let ticks = end.zip(self.keys.front()).map(|(end, key)| end.saturating_sub(key.pts).max(0)).unwrap_or(0);
        Duration::from_nanos(ticks as u64 * 100)
    }

    pub fn clear(&mut self) {
        for stream in &mut self.streams {
            stream.clear();
        }
        self.keys.clear();
        self.ends = [None; STREAMS];
        self.count = 0;
    }

    fn discard_first_gop(&mut self) {
        self.keys.pop_front();
        if let Some(key) = self.keys.front() {
            for (index, stream) in self.streams.iter_mut().enumerate() {
                while stream.front().is_some_and(
                    |p| {
                        if index == 0 {
                            p.info.dts < key.dts
                        } else {
                            p.info.pts < key.pts
                        }
                    },
                ) {
                    stream.pop_front();
                    self.count -= 1;
                }
            }
        }
    }

    pub fn push(&mut self, info: PacketInfo, data: Vec<u8>) -> PushResult {
        if info.stream >= STREAMS || info.duration <= 0 || data.is_empty() {
            return PushResult::Invalid;
        }
        let video_key = info.stream == 0 && info.keyframe;
        if self.keys.is_empty() && !video_key {
            return PushResult::WaitingForKeyframe;
        }
        if self.keys.front().is_some_and(|key| if info.stream == 0 { info.dts < key.dts } else { info.pts < key.pts }) {
            return PushResult::WaitingForKeyframe;
        }

        // Evict complete GOPs first. If a save owns their payloads, those bytes stay charged.
        while self.keys.len() > 1
            && (self.live_bytes().saturating_add(data.len()) > self.max_bytes || self.count >= MAX_PACKETS)
        {
            self.memory_evictions += 1;
            self.discard_first_gop();
        }
        if self.live_bytes().saturating_add(data.len()) > self.max_bytes || self.count >= MAX_PACKETS {
            self.clear();
            self.memory_drops += 1;
            if !video_key || self.live_bytes().saturating_add(data.len()) > self.max_bytes {
                return PushResult::MemoryLimited;
            }
        }
        self.live_bytes.fetch_add(data.len(), Ordering::Relaxed);
        let payload = Arc::new(Payload { bytes: data.into_boxed_slice(), live_bytes: self.live_bytes.clone() });
        if video_key {
            self.keys.push_back(Keyframe { pts: info.pts, dts: info.dts });
        }
        self.ends[info.stream] =
            Some(self.ends[info.stream].unwrap_or(i64::MIN).max(info.pts.saturating_add(info.duration)));
        self.streams[info.stream].push_back(Packet { info, data: payload });
        self.count += 1;
        if let Some(end) = self.ends[0] {
            while self.keys.len() > 1 && end.saturating_sub(self.keys[1].pts) >= self.window {
                self.discard_first_gop();
            }
        }
        PushResult::Stored
    }

    /// End at the latest common A/V watermark so an asynchronous encoder cannot expose
    /// a video tail whose audio has not arrived. Keep the keyframe preceding the cutoff.
    pub fn snapshot(&self, duration: Duration) -> Option<Snapshot> {
        let end = self.ends.iter().copied().collect::<Option<Vec<_>>>()?.into_iter().min()?;
        let wanted = (duration.as_nanos() / 100).min(i64::MAX as u128) as i64;
        let cutoff = end.saturating_sub(wanted);
        let key = self.keys.iter().rev().find(|k| k.pts <= cutoff).or_else(|| self.keys.front())?;
        if end <= key.pts {
            return None;
        }
        let streams: [Vec<Packet>; STREAMS] = std::array::from_fn(|index| {
            self.streams[index]
                .iter()
                .filter(|packet| {
                    let start_ok = if index == 0 { packet.info.dts >= key.dts } else { packet.info.pts >= key.pts };
                    // Video must remain a decode-order prefix: reordered B-frames can need
                    // a reference picture whose presentation time lies beyond the watermark.
                    let end_ok = if index == 0 { packet.info.dts < end } else { packet.info.pts < end };
                    start_ok && end_ok
                })
                .cloned()
                .collect()
        });
        if streams.iter().any(Vec::is_empty) || !streams[0].first()?.info.keyframe {
            return None;
        }
        Some(Snapshot { streams, start: key.pts, end })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(stream: usize, ms: i64, key: bool) -> PacketInfo {
        PacketInfo { stream, pts: ms * 10_000, dts: ms * 10_000, duration: 1_000_000, keyframe: key }
    }
    fn feed(buffer: &mut RingBuffer, frames: std::ops::Range<i64>) {
        for i in frames {
            for stream in 0..STREAMS {
                buffer.push(frame(stream, i * 100, i % 10 == 0), vec![1; 10]);
            }
        }
    }
    #[test]
    fn history_starts_on_a_keyframe_and_preserves_all_tracks() {
        let mut buffer = RingBuffer::new(Duration::from_secs(2), 100_000);
        feed(&mut buffer, 0..100);
        let snapshot = buffer.snapshot(Duration::from_secs(2)).unwrap();
        assert!(snapshot.streams[0][0].info.keyframe);
        assert!((Duration::from_secs(2)..=Duration::from_secs(3)).contains(&snapshot.duration()));
        for stream in &snapshot.streams {
            assert_eq!(stream.first().unwrap().info.pts, snapshot.start);
        }
    }
    #[test]
    fn snapshot_keeps_bytes_charged_and_memory_pressure_waits_for_a_keyframe() {
        let mut buffer = RingBuffer::new(Duration::from_secs(10), 400);
        feed(&mut buffer, 0..10);
        let snapshot = buffer.snapshot(Duration::from_secs(1)).unwrap();
        assert_eq!(buffer.live_bytes(), 400);
        assert_eq!(buffer.push(frame(0, 1000, true), vec![2; 10]), PushResult::MemoryLimited);
        assert!(buffer.snapshot(Duration::from_secs(1)).is_none());
        assert_eq!(buffer.live_bytes(), 400, "save must retain its payload without escaping the limit");
        drop(snapshot);
        assert_eq!(buffer.live_bytes(), 0);
        assert_eq!(buffer.push(frame(0, 1100, false), vec![2; 10]), PushResult::WaitingForKeyframe);
        feed(&mut buffer, 20..30);
        assert!(buffer.snapshot(Duration::from_secs(1)).is_some());
        assert!(buffer.live_bytes() <= buffer.max_bytes());
    }
    #[test]
    fn delayed_audio_limits_snapshot_end_and_dts_is_preserved() {
        let mut buffer = RingBuffer::new(Duration::from_secs(10), 100_000);
        feed(&mut buffer, 0..10);
        buffer.push(PacketInfo { pts: 10_000_000, dts: 9_000_000, ..frame(0, 1000, true) }, vec![1; 10]);
        let snapshot = buffer.snapshot(Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.end, 10_000_000);
        assert_eq!(
            snapshot.streams[0].len(),
            11,
            "retain references within the decode watermark even if their PTS is later"
        );
        assert_eq!(snapshot.streams[0].last().unwrap().info.dts, 9_000_000);
        assert_eq!(buffer.streams[0].back().unwrap().info.dts, 9_000_000);
    }
    #[test]
    fn invalid_oversized_and_non_key_packets_cannot_create_unbounded_history() {
        let mut buffer = RingBuffer::new(Duration::from_secs(10), 10);
        assert_eq!(buffer.push(frame(4, 0, true), vec![1]), PushResult::Invalid);
        assert_eq!(buffer.push(frame(0, 0, false), vec![1]), PushResult::WaitingForKeyframe);
        assert_eq!(buffer.push(frame(0, 0, true), vec![1; 11]), PushResult::MemoryLimited);
        assert_eq!(buffer.live_bytes(), 0);
    }

    #[test]
    fn budget_trimming_shortens_history_only_at_decodable_boundaries() {
        let mut buffer = RingBuffer::new(Duration::from_secs(120), 800);
        feed(&mut buffer, 0..100);
        let snapshot = buffer.snapshot(Duration::from_secs(120)).unwrap();
        assert!(snapshot.streams[0][0].info.keyframe);
        assert!(snapshot.duration() < Duration::from_secs(120));
        assert!(buffer.memory_evictions() > 0);
        assert_eq!(buffer.memory_drops(), 0);
        assert!(buffer.live_bytes() <= 800);
    }

    #[test]
    fn resize_clear_preserves_pending_save_and_its_budget_charge() {
        let mut buffer = RingBuffer::new(Duration::from_secs(2), 1000);
        feed(&mut buffer, 0..10);
        let old_save = buffer.snapshot(Duration::from_secs(1)).unwrap();
        buffer.clear();
        assert_eq!(buffer.live_bytes(), 400);
        feed(&mut buffer, 0..10); // timestamps can restart for a new size epoch
        assert_eq!(buffer.live_bytes(), 800);
        assert_eq!(old_save.streams[0][0].data.bytes(), &[1; 10]);
        drop(old_save);
        assert_eq!(buffer.live_bytes(), 400);
    }

    #[test]
    fn reordered_video_keeps_future_reference_frames_and_negative_dts() {
        let mut buffer = RingBuffer::new(Duration::from_secs(2), 10_000);
        for (pts, dts, keyframe) in [(0, -200, true), (900, -100, false), (100, 0, false), (200, 100, false)] {
            buffer.push(PacketInfo { pts: pts * 10_000, dts: dts * 10_000, ..frame(0, 0, keyframe) }, vec![1]);
        }
        for i in 0..8 {
            for stream in 1..STREAMS {
                buffer.push(frame(stream, i * 100, false), vec![1]);
            }
        }
        let snapshot = buffer.snapshot(Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.end, 8_000_000);
        assert_eq!(snapshot.streams[0].len(), 4);
        assert_eq!(snapshot.streams[0][0].info.dts, -2_000_000);
        assert_eq!(
            snapshot.streams[0][1].info.pts, 9_000_000,
            "keep the future reference needed by the following B-frames"
        );
    }
}
