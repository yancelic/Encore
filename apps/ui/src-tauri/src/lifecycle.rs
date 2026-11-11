//! Coordinate library exit with explicit export jobs. Read-only polling never holds exit open.
use std::sync::{Arc, Condvar, Mutex, OnceLock};

#[derive(Default)]
struct State {
    epoch: u64,
    closing: Option<u64>,
    exports: usize,
    exiting: bool,
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

impl Drop for ExportGuard {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.exports -= 1;
        self.0.changed.notify_all();
    }
}

impl Lifecycle {
    pub fn begin_export(self: &Arc<Self>) -> Result<ExportGuard, String> {
        let mut state = self.state.lock().unwrap();
        if state.closing.is_some() || state.exiting {
            return Err("Encore library is closing".into());
        }
        state.exports += 1;
        Ok(ExportGuard(self.clone()))
    }

    pub fn request_close(&self) -> Option<u64> {
        let mut state = self.state.lock().unwrap();
        if state.closing.is_some() || state.exiting {
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
        if state.exiting {
            return false;
        }
        state.closing = None;
        self.changed.notify_all();
        true
    }

    pub fn is_closing(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.closing.is_some() || state.exiting
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
}
