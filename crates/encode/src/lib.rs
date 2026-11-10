//! Ring buffer of encoded packets. Holds the last N seconds, starting at a keyframe.
use std::collections::VecDeque;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Packet {
    pub pts: Duration,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

pub struct RingBuffer {
    max_len: Duration,
    packets: VecDeque<Packet>,
}

impl RingBuffer {
    pub fn new(max_len: Duration) -> Self {
        Self { max_len, packets: VecDeque::new() }
    }

    pub fn push(&mut self, p: Packet) {
        let newest = p.pts;
        self.packets.push_back(p);
        // Drop packets older than the window, but only up to a keyframe so
        // that the buffer always starts at a decodable point.
        while self.packets.len() > 1 {
            let oldest = self.packets[0].pts;
            if newest.saturating_sub(oldest) <= self.max_len {
                break;
            }
            // Only drop the head if the next keyframe is within reach.
            match self.packets.iter().skip(1).position(|p| p.keyframe) {
                // Dropping must still leave at least `max_len` of footage.
                Some(i) if newest.saturating_sub(self.packets[i + 1].pts) >= self.max_len => {
                    self.packets.drain(..=i);
                }
                _ => break,
            }
        }
    }

    /// Snapshot of the buffer starting at the first keyframe.
    pub fn snapshot(&self) -> Vec<Packet> {
        match self.packets.iter().position(|p| p.keyframe) {
            Some(i) => self.packets.iter().skip(i).cloned().collect(),
            None => Vec::new(),
        }
    }

    pub fn len(&self) -> usize { self.packets.len() }
    pub fn is_empty(&self) -> bool { self.packets.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkt(ms: u64, key: bool) -> Packet {
        Packet { pts: Duration::from_millis(ms), keyframe: key, data: vec![0] }
    }

    #[test]
    fn trims_to_window_and_starts_on_keyframe() {
        let mut rb = RingBuffer::new(Duration::from_secs(2));
        for i in 0..100u64 {
            rb.push(pkt(i * 100, i % 10 == 0)); // 10s, keyframe each 1s
        }
        let snap = rb.snapshot();
        assert!(snap[0].keyframe);
        let span = snap.last().unwrap().pts - snap[0].pts;
        assert!(span >= Duration::from_secs(2) && span <= Duration::from_secs(3));
    }

    #[test]
    fn empty_without_keyframe() {
        let mut rb = RingBuffer::new(Duration::from_secs(1));
        rb.push(pkt(0, false));
        assert!(rb.snapshot().is_empty());
    }
}
