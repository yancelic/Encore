//! Opt-in real-process smoke test. Isolated config selects only our generated window.
use encore_common::ipc::{self, IpcRequest, IpcResponse, RecorderState, RecorderStatus};
use encore_common::Config;
use std::collections::HashSet;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
use windows::Win32::UI::WindowsAndMessaging::*;

struct Process {
    pid: u32,
    parent: u32,
    name: String,
}

fn processes() -> Vec<Process> {
    unsafe {
        let handle = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).unwrap();
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut result = Vec::new();
        if Process32FirstW(handle, &mut entry).is_ok() {
            loop {
                let end = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                result.push(Process {
                    pid: entry.th32ProcessID,
                    parent: entry.th32ParentProcessID,
                    name: String::from_utf16_lossy(&entry.szExeFile[..end]),
                });
                if Process32NextW(handle, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(handle);
        result
    }
}

fn wait_for(timeout: u64, mut condition: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(timeout);
    while Instant::now() < end {
        if condition() {
            return true;
        }
        thread::sleep(Duration::from_millis(30));
    }
    false
}

fn alive(pid: u32) -> bool {
    processes().iter().any(|p| p.pid == pid)
}

fn status() -> Option<RecorderStatus> {
    match ipc::send_request(&IpcRequest::GetStatus).ok()? {
        IpcResponse::Status(status) => Some(status),
        _ => None,
    }
}

fn command(binary: &Path, root: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("APPDATA", root.join("roaming"))
        .env("LOCALAPPDATA", root.join("local"))
        .env("WEBVIEW2_USER_DATA_FOLDER", root.join("webview"));
    command
}

fn webviews(ui: u32) -> Vec<u32> {
    let snapshot = processes();
    let mut family = HashSet::from([ui]);
    loop {
        let before = family.len();
        for process in &snapshot {
            if family.contains(&process.parent) {
                family.insert(process.pid);
            }
        }
        if family.len() == before {
            break;
        }
    }
    snapshot
        .iter()
        .filter(|p| family.contains(&p.pid) && p.name.eq_ignore_ascii_case("msedgewebview2.exe"))
        .map(|p| p.pid)
        .collect()
}

struct OwnedProcesses {
    recorder: Child,
    libraries: Vec<u32>,
}

impl Drop for OwnedProcesses {
    fn drop(&mut self) {
        // Cleanup is restricted to UI children of this test's recorder. Never terminate
        // a pre-existing user UI/recorder, or a game process.
        for process in processes().iter().filter(|p| p.parent == self.recorder.id() && self.libraries.contains(&p.pid))
        {
            if let Some(window) = library_window(process.pid) {
                unsafe {
                    let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            if !wait_for(3, || !alive(process.pid)) {
                unsafe {
                    if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, process.pid) {
                        let _ = TerminateProcess(handle, 1);
                        let _ = CloseHandle(handle);
                    }
                }
            }
        }
        if self.recorder.try_wait().ok().flatten().is_none() {
            let _ = ipc::send_request(&IpcRequest::Shutdown);
            if !wait_for(5, || self.recorder.try_wait().ok().flatten().is_some()) {
                let _ = self.recorder.kill();
            }
        }
        let _ = self.recorder.wait();
    }
}

fn library_window(pid: u32) -> Option<HWND> {
    struct Search {
        pid: u32,
        result: Option<HWND>,
    }
    unsafe extern "system" fn callback(window: HWND, context: LPARAM) -> windows::core::BOOL {
        let search = unsafe { &mut *(context.0 as *mut Search) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, Some(&mut pid));
        }
        if pid == search.pid {
            let mut title = [0u16; 64];
            let len = unsafe { GetWindowTextW(window, &mut title) };
            if String::from_utf16_lossy(&title[..len as usize]) == "Encore" {
                search.result = Some(window);
                return false.into();
            }
        }
        true.into()
    }
    let mut search = Search { pid, result: None };
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(&mut search as *mut Search as isize));
    }
    search.result
}

fn native_tray_command(owner: &OwnedProcesses, command: usize) {
    let window =
        unsafe { FindWindowExW(Some(HWND_MESSAGE), None, w!("EncoreRecorderMsgWin"), w!("EncoreRecorder")).unwrap() };
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(window, Some(&mut pid));
    }
    assert_eq!(pid, owner.recorder.id());
    unsafe {
        PostMessageW(Some(window), WM_APP + 21, WPARAM(command), LPARAM(0)).unwrap();
    }
}

fn open_library(owner: &mut OwnedProcesses) -> u32 {
    native_tray_command(owner, crate::tray::CMD_OPEN_UI);
    let mut found = None;
    assert!(
        wait_for(15, || {
            found = processes()
                .iter()
                .find(|p| p.parent == owner.recorder.id() && p.name.eq_ignore_ascii_case("encore-ui.exe"))
                .map(|p| p.pid);
            found.is_some_and(|pid| library_window(pid).is_some())
        }),
        "native tray did not create the library process/window"
    );
    let pid = found.unwrap();
    owner.libraries.push(pid);
    pid
}

#[test]
#[ignore = "requires release binaries, an interactive Windows desktop and permission for own-window capture/WebView2"]
fn native_library_close_releases_webview_and_keeps_recording() {
    assert!(status().is_none(), "close existing Encore before running this isolated smoke test");
    assert!(
        !processes().iter().any(|p| p.name.eq_ignore_ascii_case("encore-ui.exe")),
        "close the existing Encore library first"
    );
    let release = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/release");
    let recorder = release.join("encore-recorder.exe");
    assert!(recorder.exists() && release.join("encore-ui.exe").exists(), "build --release --workspace first");
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("encore_library_lifecycle_{}_{}", std::process::id(), stamp));
    let config_dir = root.join("roaming/Encore");
    std::fs::create_dir_all(&config_dir).unwrap();
    let fixture = crate::test_window::TestWindow::new();
    let test_exe = std::env::current_exe().unwrap();
    let config = Config {
        games: vec![test_exe.file_name().unwrap().to_string_lossy().into_owned()],
        clips_dir: root.join("clips"),
        buffer_secs: 2,
        fps: 60,
        height: 720,
        audio_device: Some("off".into()),
        chat_device: Some("off".into()),
        mic_device: Some("off".into()),
        hotkey: "Ctrl+Alt+Shift+F24".into(),
        ..Config::default()
    };
    std::fs::write(config_dir.join("config.json"), serde_json::to_vec(&config).unwrap()).unwrap();
    let mut owner = OwnedProcesses { recorder: command(&recorder, &root).spawn().unwrap(), libraries: Vec::new() };
    assert!(
        wait_for(10, || status().is_some_and(|s| s.state == RecorderState::Recording)),
        "isolated fixture recording did not start"
    );
    let first = open_library(&mut owner);
    let mut first_webviews = Vec::new();
    assert!(
        wait_for(10, || {
            first_webviews = webviews(first);
            !first_webviews.is_empty()
        }),
        "library did not create WebView2"
    );
    unsafe {
        PostMessageW(Some(library_window(first).unwrap()), WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
    }
    assert!(wait_for(10, || !alive(first)), "closing the library only hid its process");
    assert!(wait_for(10, || first_webviews.iter().all(|&pid| !alive(pid))), "library WebView2 children remained alive");
    assert!(owner.recorder.try_wait().unwrap().is_none(), "UI close stopped the recorder");
    assert_eq!(status().unwrap().state, RecorderState::Recording);
    assert!(matches!(ipc::send_request(&IpcRequest::SaveClip).unwrap(), IpcResponse::Ok));
    assert!(wait_for(10, || status().is_some_and(|s| s.last_clip.is_some())), "headless hotkey/IPC save failed");
    let saved = PathBuf::from(status().unwrap().last_clip.unwrap());
    assert!(saved.starts_with(&root) && saved.exists(), "test must use its own config and clip directory");
    let second = open_library(&mut owner);
    assert_ne!(first, second);
    let mut duplicate = command(&release.join("encore-ui.exe"), &root).spawn().unwrap();
    assert!(wait_for(8, || duplicate.try_wait().unwrap().is_some()), "duplicate launch did not reuse the library");
    assert!(alive(second));
    let second_webviews = webviews(second);
    native_tray_command(&owner, crate::tray::CMD_QUIT);
    assert!(wait_for(10, || owner.recorder.try_wait().unwrap().is_some()), "native Quit did not stop recorder");
    assert!(wait_for(10, || !alive(second)), "native Quit did not stop library");
    assert!(wait_for(10, || second_webviews.iter().all(|&pid| !alive(pid))));
    drop(owner);
    drop(fixture);
    std::fs::remove_dir_all(root).unwrap();
}
