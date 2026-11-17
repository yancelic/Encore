//! Per-user Unix sockets. Reads and deadlines are bounded; commands are never replayed.
use crate::ipc::{AudioDeviceInfo, IpcRequest, IpcResponse, PIPE_NAME, UI_WAKE_PIPE};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

const MAX_MESSAGE: u64 = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(3);

fn runtime_dir() -> Result<PathBuf, String> {
    let uid = unsafe { libc::geteuid() };
    let mut parent = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    if let Some(app_id) = std::env::var_os("FLATPAK_ID") {
        parent = parent.join("app").join(app_id);
    }
    let directory = parent.join(format!("encore-{uid}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(format!("Could not create Encore IPC directory: {e}")),
    }
    let metadata = std::fs::symlink_metadata(&directory).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err("Encore IPC directory must be private and owned by this user".into());
    }
    Ok(directory)
}

pub fn socket_path(name: &str) -> Result<PathBuf, String> {
    let file = match name {
        PIPE_NAME => "recorder.sock",
        UI_WAKE_PIPE => "library.sock",
        _ => return Err("Unknown Encore IPC endpoint".into()),
    };
    Ok(runtime_dir()?.join(file))
}

fn configure(stream: &UnixStream) -> Result<(), String> {
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())
}

fn connect(name: &str) -> Result<UnixStream, String> {
    let path = socket_path(name)?;
    let mut last = String::new();
    for attempt in 0..10 {
        match UnixStream::connect(&path) {
            Ok(stream) => {
                configure(&stream)?;
                return Ok(stream);
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) =>
            {
                last = e.to_string();
                if attempt < 9 {
                    std::thread::sleep(Duration::from_millis(15));
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Err(format!("Recorder unavailable: {last}"))
}

pub fn read_message(stream: &mut UnixStream) -> Result<String, String> {
    use std::io::Read;
    let mut message = String::new();
    BufReader::new(stream.take(MAX_MESSAGE + 1))
        .read_line(&mut message)
        .map_err(|e| e.to_string())?;
    if message.len() as u64 > MAX_MESSAGE || !message.ends_with('\n') {
        return Err("Invalid or oversized IPC message".into());
    }
    Ok(message)
}

pub fn send_ui_command(command: &str) -> Result<bool, String> {
    if !matches!(command, "show" | "quit") {
        return Err("Invalid UI command".into());
    }
    let mut stream = connect(UI_WAKE_PIPE)?;
    writeln!(stream, "{command}").map_err(|e| e.to_string())?;
    Ok(read_message(&mut stream)?.trim() == "ok")
}

pub fn send_to_pipe(name: &str, request: &IpcRequest) -> Result<IpcResponse, String> {
    let mut stream = connect(name)?;
    let message = serde_json::to_string(request).map_err(|e| e.to_string())?;
    writeln!(stream, "{message}").map_err(|e| e.to_string())?;
    serde_json::from_str(read_message(&mut stream)?.trim()).map_err(|e| e.to_string())
}

pub fn send_request(request: &IpcRequest) -> Result<IpcResponse, String> {
    send_to_pipe(PIPE_NAME, request)
}

pub fn list_audio_devices() -> Vec<AudioDeviceInfo> {
    match send_request(&IpcRequest::ListAudioDevices) {
        Ok(IpcResponse::AudioDevices(devices)) => devices,
        _ => Vec::new(),
    }
}

/// The lock outlives the socket, so a second process cannot unlink a live endpoint.
pub struct LocalServer {
    listener: UnixListener,
    path: PathBuf,
    _lock: File,
}

impl LocalServer {
    pub fn bind(name: &str) -> Result<Self, String> {
        Self::bind_path(socket_path(name)?)
    }

    fn bind_path(path: PathBuf) -> Result<Self, String> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path.with_extension("lock"))
            .map_err(|e| e.to_string())?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("Encore is already running".into());
        }
        match std::fs::remove_file(&path) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
        let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        Ok(Self {
            listener,
            path,
            _lock: lock,
        })
    }

    pub fn accept(&self) -> Result<Option<UnixStream>, String> {
        match self.listener.accept() {
            Ok((stream, _)) => {
                configure(&stream)?;
                Ok(Some(stream))
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singleton_preserves_live_socket_and_restarts_after_drop() {
        let path = runtime_dir()
            .unwrap()
            .join(format!("test-{}.sock", std::process::id()));
        let first = LocalServer::bind_path(path.clone()).unwrap();
        assert!(LocalServer::bind_path(path.clone()).is_err());
        assert!(path.exists());
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(first);
        assert!(!path.exists());
        let _second = LocalServer::bind_path(path).unwrap();
    }

    #[test]
    fn protocol_rejects_truncated_and_oversized_messages() {
        use std::net::Shutdown;
        for bytes in [b"no newline".to_vec(), vec![b'x'; MAX_MESSAGE as usize + 1]] {
            let (mut reader, mut writer) = UnixStream::pair().unwrap();
            let sender = std::thread::spawn(move || {
                let _ = writer.write_all(&bytes);
                let _ = writer.shutdown(Shutdown::Write);
            });
            assert!(read_message(&mut reader).is_err());
            drop(reader);
            sender.join().unwrap();
        }
    }

    #[test]
    fn json_request_response_roundtrip_over_unix_socket() {
        let (mut reader, mut writer) = UnixStream::pair().unwrap();
        let server = std::thread::spawn(move || {
            assert!(matches!(
                serde_json::from_str::<IpcRequest>(read_message(&mut reader).unwrap().trim())
                    .unwrap(),
                IpcRequest::SaveClip
            ));
            writeln!(
                reader,
                "{}",
                serde_json::to_string(&IpcResponse::Ok).unwrap()
            )
            .unwrap();
        });
        writeln!(
            writer,
            "{}",
            serde_json::to_string(&IpcRequest::SaveClip).unwrap()
        )
        .unwrap();
        assert!(matches!(
            serde_json::from_str::<IpcResponse>(read_message(&mut writer).unwrap().trim()).unwrap(),
            IpcResponse::Ok
        ));
        server.join().unwrap();
    }
}
