//! One save worker plus one pending job; capture never waits for disk/FFmpeg.
use crate::encoder::{self, MfGpu, ReplaySnapshot};
use encore_common::ipc::{RecorderState, RecorderStatus};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

struct SaveJob {
    snapshot: ReplaySnapshot,
    output: PathBuf,
}

pub struct SaveWorker {
    sender: Option<SyncSender<SaveJob>>,
    pending: Arc<AtomicUsize>,
    status: Arc<Mutex<RecorderStatus>>,
    thread: Option<JoinHandle<()>>,
}

impl SaveWorker {
    pub fn new(gpu: Arc<MfGpu>, game: Option<String>, status: Arc<Mutex<RecorderStatus>>) -> Self {
        Self::with_executor(status, move |snapshot, output| {
            let _gpu_lifetime = &gpu;
            crate::clip::save_replay(snapshot, output, game.as_deref())
        })
    }

    fn with_executor(
        status: Arc<Mutex<RecorderStatus>>,
        execute: impl Fn(ReplaySnapshot, &Path) -> Result<(), String> + Send + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<SaveJob>(1);
        let pending = Arc::new(AtomicUsize::new(0));
        let thread_pending = pending.clone();
        let thread_status = status.clone();
        let thread = thread::spawn(move || {
            encoder::ensure_com_mta();
            while let Ok(job) = receiver.recv() {
                let result = execute(job.snapshot, &job.output);
                thread_pending.fetch_sub(1, Ordering::AcqRel);
                let mut status = thread_status.lock().unwrap();
                match result {
                    Ok(()) => {
                        println!("Saved RAM replay: {}", job.output.display());
                        status.last_clip = Some(job.output.to_string_lossy().into_owned());
                        status.error = None;
                    }
                    Err(error) => {
                        eprintln!("Saving RAM replay failed: {error}");
                        status.error = Some(error);
                    }
                }
                if thread_pending.load(Ordering::Acquire) == 0 {
                    status.state = if status.target.is_some() { RecorderState::Recording } else { RecorderState::Idle };
                }
            }
        });
        Self { sender: Some(sender), pending, status, thread: Some(thread) }
    }

    pub fn pending(&self) -> usize {
        self.pending.load(Ordering::Acquire)
    }

    pub fn submit(&self, snapshot: ReplaySnapshot, output: PathBuf) -> Result<(), String> {
        let mut status = self.status.lock().unwrap();
        self.pending.fetch_add(1, Ordering::AcqRel);
        match self.sender.as_ref().unwrap().try_send(SaveJob { snapshot, output }) {
            Ok(()) => {
                status.state = RecorderState::Saving;
                Ok(())
            }
            Err(error) => {
                self.pending.fetch_sub(1, Ordering::AcqRel);
                Err(format!(
                    "Replay save queue is busy; wait for the current clips to finish ({})",
                    match error {
                        mpsc::TrySendError::Full(_) => "queue full",
                        mpsc::TrySendError::Disconnected(_) => "worker stopped",
                    }
                ))
            }
        }
    }
}

impl Drop for SaveWorker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                eprintln!("Replay save worker stopped unexpectedly");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn snapshot() -> ReplaySnapshot {
        ReplaySnapshot {
            packets: encore_encode::Snapshot { streams: std::array::from_fn(|_| Vec::new()), start: 0, end: 1 },
            formats: Vec::new(),
        }
    }

    #[test]
    fn slow_save_has_only_one_pending_job_and_never_blocks_capture_submission() {
        let status = Arc::new(Mutex::new(RecorderStatus {
            state: RecorderState::Recording,
            target: Some("test.exe".into()),
            mode: encore_common::CaptureMode::GameDetection,
            fps: 60,
            height: 1080,
            buffer_secs: 60,
            hotkey: "Alt+S".into(),
            last_clip: None,
            error: None,
            audio_warnings: Default::default(),
        }));
        let (started_tx, started_rx) = mpsc::channel();
        let (permit_tx, permit_rx) = mpsc::channel();
        let worker = SaveWorker::with_executor(status.clone(), move |_, _| {
            started_tx.send(()).unwrap();
            permit_rx.recv_timeout(Duration::from_secs(3)).map_err(|e| e.to_string())
        });
        worker.submit(snapshot(), PathBuf::from("first.mp4")).unwrap();
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        worker.submit(snapshot(), PathBuf::from("second.mp4")).unwrap();
        let began = Instant::now();
        let rejected = worker.submit(snapshot(), PathBuf::from("third.mp4"));
        let elapsed = began.elapsed();
        let pending = worker.pending();
        permit_tx.send(()).unwrap();
        permit_tx.send(()).unwrap();
        drop(worker); // drains both accepted saves
        assert!(rejected.is_err());
        assert!(elapsed < Duration::from_secs(1), "a full save queue must not wait for disk");
        assert_eq!(pending, 2);
        let status = status.lock().unwrap();
        assert_eq!(status.last_clip.as_deref(), Some("second.mp4"));
        assert_eq!(status.state, RecorderState::Recording);
        assert!(status.error.is_none());
    }
}
