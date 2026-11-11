//! Library process lifecycle and single-instance wake. The recorder owns the only tray icon.
use encore_common::ipc::{self, UI_WAKE_PIPE};
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;
use tauri::{AppHandle, Manager};

#[cfg(windows)]
pub struct SingleInstanceGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
pub fn acquire_single_instance() -> Option<SingleInstanceGuard> {
    use windows::core::w;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    // An exiting UI can still hold the singleton briefly. Retry so a tray click is
    // not lost between its final wake acknowledgement and the OS closing the process.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        unsafe {
            let handle = CreateMutexW(None, false, w!("Local\\EncoreUiSingleton")).ok()?;
            if GetLastError() != ERROR_ALREADY_EXISTS {
                return Some(SingleInstanceGuard(handle));
            }
            let _ = CloseHandle(handle);
        }
        if ipc::send_ui_command("show").unwrap_or(false) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[cfg(not(windows))]
pub fn acquire_single_instance() -> Option<()> {
    Some(())
}

pub fn show_main_window(app: &AppHandle) -> bool {
    if !crate::lifecycle::global().reopen() {
        return false;
    }
    let Some(window) = app.get_webview_window("main") else { return false };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    true
}

/// Hide the library while accepted exports drain, then exit the process and release
/// WebView2. The recorder, its hotkey and native tray are independent of this process.
pub fn close_library(app: &AppHandle) {
    let lifecycle = crate::lifecycle::global().clone();
    let Some(epoch) = lifecycle.request_close() else { return };
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if lifecycle.wait_for_exports(epoch) {
            let exit_app = app.clone();
            let _ = app.run_on_main_thread(move || {
                if lifecycle.commit_exit(epoch) {
                    exit_app.exit(0);
                }
            });
        }
    });
}

#[cfg(windows)]
pub fn start_wake_listener(app: AppHandle) {
    use std::os::windows::io::FromRawHandle;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
    use windows::Win32::Storage::FileSystem::{FlushFileBuffers, PIPE_ACCESS_DUPLEX};
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    std::thread::spawn(move || {
        let name = HSTRING::from(UI_WAKE_PIPE);
        loop {
            let handle = unsafe {
                CreateNamedPipeW(
                    &name,
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    2,
                    256,
                    256,
                    0,
                    None,
                )
            };
            if handle == INVALID_HANDLE_VALUE || handle.is_invalid() {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            let connected = unsafe {
                match ConnectNamedPipe(handle, None) {
                    Ok(()) => true,
                    Err(e) => e.code().0 as u32 == ERROR_PIPE_CONNECTED.0,
                }
            };
            if !connected {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                continue;
            }
            let file = unsafe { std::fs::File::from_raw_handle(handle.0) };
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            if reader.read_line(&mut line).is_ok() {
                let accepted = match line.trim() {
                    "show" => show_main_window(&app),
                    "quit" => {
                        close_library(&app);
                        true
                    }
                    _ => false,
                };
                let file = reader.get_mut();
                let _ = writeln!(file, "{}", if accepted { "ok" } else { "closing" });
                unsafe {
                    let _ = FlushFileBuffers(handle);
                }
            }
            unsafe {
                let _ = DisconnectNamedPipe(handle);
            }
        }
    });
}
