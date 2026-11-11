//! Coordinate library exit with explicit export jobs. Read-only polling never holds exit open.
use std::sync::{Arc, Condvar, Mutex, OnceLock};

#[derive(Default)]
struct State {
    epoch: u64,
    closing: Option<u64>,
    exports: usize,
    tasks: usize,
    exiting: bool,
    updating: bool,
}

#[derive(Default)]
pub struct Lifecycle {
    state: Mutex<State>,
    changed: Condvar,
}

pub fn global() -> &'static Arc<Lifecycle> {
    static LIFECYCLE: OnceLock<Arc<Lifecycle>> = OnceLock::new();
    LIFECYCLE.get_or_init(|| Arc::new(Lifecycle::default()))
}

pub struct ExportGuard(Arc<Lifecycle>);
pub struct TaskGuard(Arc<Lifecycle>);

impl Drop for TaskGuard {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.tasks -= 1;
        self.0.changed.notify_all();
    }
}

/// Holds the UI open and rejects new work until installation or recovery.
pub struct UpdateGuard(Arc<Lifecycle>);

impl UpdateGuard {
    pub fn wait_for_jobs(&self) {
        let mut state = self.0.state.lock().unwrap();
        while state.exports != 0 || state.tasks != 0 {
            state = self.0.changed.wait(state).unwrap();
        }
    }
}

impl Drop for UpdateGuard {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().updating = false;
        self.0.changed.notify_all();
    }
}

impl Drop for ExportGuard {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.exports -= 1;
        self.0.changed.notify_all();
    }
}

impl Lifecycle {
    /// Read jobs do not block a normal close, but may own bundled FFmpeg during an update.
    pub fn begin_task(self: &Arc<Self>) -> Result<TaskGuard, String> {
        let mut state = self.state.lock().unwrap();
        if state.closing.is_some() || state.exiting || state.updating {
            return Err("Encore is preparing to close or update".into());
        }
        state.tasks += 1;
        Ok(TaskGuard(self.clone()))
    }

    pub fn begin_update(self: &Arc<Self>) -> Result<UpdateGuard, String> {
        let mut state = self.state.lock().unwrap();
        if state.closing.is_some() || state.exiting || state.updating {
            return Err("Encore is closing or already updating. Try again shortly.".into());
        }
        state.updating = true;
        Ok(UpdateGuard(self.clone()))
    }

    pub fn ensure_available(&self) -> Result<(), String> {
        let state = self.state.lock().unwrap();
        if state.updating || state.exiting || state.closing.is_some() {
            Err("Encore is preparing to close or update".into())
        } else {
            Ok(())
        }
    }

    pub fn begin_export(self: &Arc<Self>) -> Result<ExportGuard, String> {
        let mut state = self.state.lock().unwrap();
        if state.closing.is_some() || state.exiting || state.updating {
            return Err("Encore library is closing".into());
        }
        state.exports += 1;
        Ok(ExportGuard(self.clone()))
    }

    pub fn request_close(&self) -> Option<u64> {
        let mut state = self.state.lock().unwrap();
        if state.closing.is_some() || state.exiting || state.updating {
            return None;
        }
        state.epoch += 1;
        let epoch = state.epoch;
        state.closing = Some(epoch);
        self.changed.notify_all();
        Some(epoch)
    }

    pub fn reopen(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.exiting || state.updating {
            return false;
        }
        state.closing = None;
        self.changed.notify_all();
        true
    }

    pub fn wait_for_exports(&self, epoch: u64) -> bool {
        let mut state = self.state.lock().unwrap();
        while state.closing == Some(epoch) && state.exports != 0 {
            state = self.changed.wait(state).unwrap();
        }
        state.closing == Some(epoch) && !state.exiting
    }

    pub fn commit_exit(&self, epoch: u64) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.closing != Some(epoch) || state.exports != 0 || state.exiting {
            return false;
        }
        state.exiting = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn update_waits_for_exports_blocks_close_and_recovers_after_failure() {
        let lifecycle = Arc::new(Lifecycle::default());
        let export = lifecycle.begin_export().unwrap();
        let preview = lifecycle.begin_task().unwrap();
        let update = lifecycle.begin_update().unwrap();
        assert!(lifecycle.begin_update().is_err());
        assert!(lifecycle.begin_export().is_err());
        assert!(lifecycle.begin_task().is_err());
        assert!(lifecycle.ensure_available().is_err());
        assert!(lifecycle.request_close().is_none());
        assert!(!lifecycle.reopen());
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            update.wait_for_jobs();
            tx.send(()).unwrap();
            // Simulate an installation error releasing the guard.
        });
        assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
        drop(export);
        assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
        drop(preview);
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        thread.join().unwrap();
        assert!(lifecycle.ensure_available().is_ok());
        assert!(lifecycle.begin_export().is_ok());
        assert!(lifecycle.request_close().is_some());
    }

    #[test]
    fn close_waits_for_accepted_export_but_rejects_new_jobs() {
        let lifecycle = Arc::new(Lifecycle::default());
        let export = lifecycle.begin_export().unwrap();
        let epoch = lifecycle.request_close().unwrap();
        assert!(lifecycle.begin_export().is_err());
        assert!(!lifecycle.commit_exit(epoch));
        let waiting = lifecycle.clone();
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            tx.send(waiting.wait_for_exports(epoch)).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
        drop(export);
        assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap());
        thread.join().unwrap();
        assert!(lifecycle.commit_exit(epoch));
        assert!(!lifecycle.reopen());
    }

    #[test]
    fn reopening_cancels_old_close_even_when_exports_are_still_running() {
        let lifecycle = Arc::new(Lifecycle::default());
        let export = lifecycle.begin_export().unwrap();
        let first = lifecycle.request_close().unwrap();
        assert!(lifecycle.reopen());
        assert!(!lifecycle.wait_for_exports(first));
        let second = lifecycle.request_close().unwrap();
        drop(export);
        assert!(!lifecycle.commit_exit(first));
        assert!(lifecycle.wait_for_exports(second));
        assert!(lifecycle.commit_exit(second));
    }

    #[test]
    fn idle_close_can_exit_immediately_and_duplicate_close_is_ignored() {
        let lifecycle = Lifecycle::default();
        let epoch = lifecycle.request_close().unwrap();
        assert!(lifecycle.request_close().is_none());
        assert!(lifecycle.wait_for_exports(epoch));
        assert!(lifecycle.commit_exit(epoch));
    }

    #[test]
    fn normal_close_does_not_wait_for_read_only_work() {
        let lifecycle = Arc::new(Lifecycle::default());
        let _preview = lifecycle.begin_task().unwrap();
        let epoch = lifecycle.request_close().unwrap();
        assert!(lifecycle.wait_for_exports(epoch));
        assert!(lifecycle.commit_exit(epoch));
    }
}
