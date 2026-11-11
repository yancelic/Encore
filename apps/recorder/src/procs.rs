//! Process-name lookup that never opens a handle to another process.
//!
//! A toolhelp snapshot (the same data Task Manager lists) gives pid -> exe name without
//! `OpenProcess`, so Encore never requests read/query access on a game process.
use std::collections::HashMap;

/// Snapshot of running processes: pid -> executable file name.
pub fn snapshot() -> HashMap<u32, String> {
    encore_common::processes::snapshot().into_iter().map(|p| (p.pid, p.exe)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_contains_this_process() {
        let me = std::process::id();
        let procs = snapshot();
        assert!(procs.get(&me).is_some_and(|n| n.to_lowercase().contains("encore")), "{:?}", procs.get(&me));
    }
}
