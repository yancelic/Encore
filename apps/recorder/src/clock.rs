//! High-resolution QPC clock (100-ns units) and waitable timer helpers.
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, SetWaitableTimer,
    TIMER_ALL_ACCESS, WaitForSingleObject,
};

fn qpc_freq() -> i64 {
    static FREQ: OnceLock<i64> = OnceLock::new();
    *FREQ.get_or_init(|| {
        let mut f = 0i64;
        unsafe {
            let _ = QueryPerformanceFrequency(&mut f);
        }
        f.max(1)
    })
}

/// Current QueryPerformanceCounter value in 100-nanosecond ticks.
/// Matches the clock domain of WGC `SystemRelativeTime` and WASAPI `pu64QPCPosition`.
pub fn qpc_100ns() -> i64 {
    let mut cnt = 0i64;
    unsafe {
        let _ = QueryPerformanceCounter(&mut cnt);
    }
    ((cnt as i128 * 10_000_000) / qpc_freq() as i128) as i64
}

/// Sub-millisecond Windows waitable timer (falls back to `thread::sleep` if unavailable).
pub struct HighResTimer {
    handle: Option<HANDLE>,
}

impl HighResTimer {
    pub fn new() -> Self {
        let handle = unsafe {
            CreateWaitableTimerExW(
                None,
                None,
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS.0,
            )
            .ok()
        };
        Self { handle }
    }

    pub fn sleep_until_100ns(&self, target_100ns: i64) {
        let rem = target_100ns - qpc_100ns();
        if rem <= 0 {
            return;
        }
        if let Some(h) = self.handle {
            let due = -rem; // negative = relative interval in 100ns units
            unsafe {
                if SetWaitableTimer(h, &due, 0, None, None, false).is_ok() {
                    let _ = WaitForSingleObject(h, 1000);
                    return;
                }
            }
        }
        thread::sleep(Duration::from_nanos((rem as u64) * 100));
    }
}

impl Drop for HighResTimer {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            unsafe {
                let _ = CloseHandle(h);
            }
        }
    }
}
