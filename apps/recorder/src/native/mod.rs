//! Native recorder supervisor. Its lifetime and bounded save worker are independent of the UI.
mod ffi;

use encore_common::ipc::{
    self, IpcRequest, IpcResponse, NativeCommand, RecorderState, RecorderStatus,
};
use encore_common::platform::CommandExt;
use encore_common::{pretty_game_name, Config};
use encore_encode::{RingBuffer, Snapshot};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const REPLAY_BUDGET: usize = 512 * 1024 * 1024;

struct Control {
    request: IpcRequest,
    reply: SyncSender<IpcResponse>,
    deadline: Instant,
}

struct SaveJob {
    packets: Snapshot,
    formats: Vec<ffi::Format>,
    path: PathBuf,
    game: String,
}

struct Saves {
    sender: Option<SyncSender<SaveJob>>,
    pending: Arc<AtomicUsize>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Saves {
    fn new(api: Arc<ffi::Api>, status: Arc<Mutex<RecorderStatus>>) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<SaveJob>(1);
        let pending = Arc::new(AtomicUsize::new(0));
        let worker_pending = pending.clone();
        let worker = thread::spawn(move || {
            while let Ok(job) = receiver.recv() {
                let destination = job.path.clone();
                let result = save_clip(&api, job);
                worker_pending.fetch_sub(1, Ordering::AcqRel);
                let mut status = status.lock().unwrap();
                match result {
                    Ok(()) => {
                        status.last_clip = Some(destination.to_string_lossy().into_owned());
                        status.error = None;
                    }
                    Err(error) => {
                        eprintln!("Native replay save failed: {error}");
                        status.error = Some(error);
                    }
                }
            }
        });
        Self {
            sender: Some(sender),
            pending,
            thread: Some(worker),
        }
    }

    fn submit(
        &self,
        engine: &ffi::Engine,
        replay: &Mutex<RingBuffer>,
        config: &Config,
        target: &str,
    ) -> Result<(), String> {
        let formats = engine.formats()?;
        let packets = replay
            .lock()
            .map_err(|_| "Replay buffer unavailable")?
            .snapshot(Duration::from_secs(config.buffer_secs as u64))
            .ok_or("Replay is warming up or waiting for a keyframe after memory pressure")?;
        let game = pretty_game_name(target);
        let stem: String = game
            .chars()
            .map(|character| {
                if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') {
                    character
                } else {
                    '_'
                }
            })
            .take(80)
            .collect();
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path = config.clips_dir.join(format!("Encore_{stem}_{time}.mp4"));
        self.pending.fetch_add(1, Ordering::AcqRel);
        if self
            .sender
            .as_ref()
            .unwrap()
            .try_send(SaveJob {
                packets,
                formats,
                path,
                game,
            })
            .is_err()
        {
            self.pending.fetch_sub(1, Ordering::AcqRel);
            return Err("Replay save queue is busy; wait for the current clips to finish".into());
        }
        Ok(())
    }
}

impl Drop for Saves {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}

fn save_clip(api: &ffi::Api, job: SaveJob) -> Result<(), String> {
    let parent = job.path.parent().ok_or("Invalid clip path")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let filename = job
        .path
        .file_name()
        .ok_or("Invalid clip filename")?
        .to_string_lossy();
    let raw = parent.join(format!(".tmp_{filename}_raw.mp4"));
    let ready = parent.join(format!(".tmp_{filename}_ready.mp4"));
    let muxed = api.mux(&raw, &job.game, &job.formats, &job.packets);
    let destination = job.path.clone();
    drop(job); // Release old packets before the disk-only faststart pass.
    let result = muxed.and_then(|()| {
        let output = Command::new(encore_common::ffmpeg_path())
            .background()
            .args(["-y", "-v", "error", "-i"])
            .arg(&raw)
            .args(["-map", "0", "-c", "copy", "-movflags", "+faststart"])
            .arg(&ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("Could not start FFmpeg: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "Clip finalization failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        std::fs::rename(&ready, &destination).map_err(|e| e.to_string())
    });
    let _ = std::fs::remove_file(raw);
    if result.is_err() {
        let _ = std::fs::remove_file(ready);
    }
    result
}

fn configuration(config: &Config) -> Result<Value, String> {
    let mut value = serde_json::to_value(config).map_err(|e| e.to_string())?;
    value
        .as_object_mut()
        .ok_or("Invalid recorder configuration")?
        .remove("games");
    value["state_dir"] = json!(Config::dir());
    value["game_targets"] = json!(config
        .games
        .iter()
        .map(|value| json!({"value": value}))
        .collect::<Vec<_>>());
    Ok(json!({"action":"configure", "config":value}))
}

fn serve(
    server: Arc<ipc::LocalServer>,
    status: Arc<Mutex<RecorderStatus>>,
    sender: SyncSender<Control>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) {
        let mut stream = match server.accept() {
            Ok(Some(stream)) => stream,
            Ok(None) => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(error) => {
                eprintln!("Recorder IPC: {error}");
                break;
            }
        };
        let response = match ipc::read_message(&mut stream)
            .and_then(|line| serde_json::from_str::<IpcRequest>(&line).map_err(|e| e.to_string()))
        {
            Ok(IpcRequest::GetStatus) => IpcResponse::Status(status.lock().unwrap().clone()),
            Ok(request) => {
                let (reply, receiver) = mpsc::sync_channel(1);
                let deadline = Instant::now() + Duration::from_secs(2);
                match sender.try_send(Control {
                    request,
                    reply,
                    deadline,
                }) {
                    Ok(()) => receiver
                        .recv_timeout(Duration::from_secs(2))
                        .unwrap_or_else(|_| IpcResponse::Err("Recorder command timed out".into())),
                    Err(_) => IpcResponse::Err("Recorder is busy; try again".into()),
                }
            }
            Err(error) => IpcResponse::Err(error),
        };
        if let Ok(json) = serde_json::to_string(&response) {
            let _ = writeln!(stream, "{json}");
        }
    }
}

fn native_data(engine: &mut ffi::Engine, command: NativeCommand) -> Result<IpcResponse, String> {
    let action = match command {
        NativeCommand::ListCaptureApplications => "applications",
        NativeCommand::ListAudioApplications => "audio_applications",
        NativeCommand::SelectCaptureSource => "select_source",
        NativeCommand::GetDiagnostics => "diagnostics",
    };
    Ok(IpcResponse::NativeData(
        engine.request(&json!({"action":action}))?,
    ))
}

fn handle_control(
    control: Control,
    engine: &mut ffi::Engine,
    replay: &Mutex<RingBuffer>,
    saves: &Saves,
    config: &mut Config,
    status: &Mutex<RecorderStatus>,
    stop: &AtomicBool,
) {
    if Instant::now() > control.deadline {
        return;
    }
    let response = (|| -> Result<IpcResponse, String> {
        match control.request {
            IpcRequest::GetStatus => Ok(IpcResponse::Status(status.lock().unwrap().clone())),
            IpcRequest::SaveClip => {
                let target = status.lock().unwrap().target.clone().unwrap_or_default();
                saves.submit(engine, replay, config, &target)?;
                Ok(IpcResponse::Ok)
            }
            IpcRequest::ReloadConfig => {
                let new_config = Config::load();
                let answer = engine.request(&configuration(&new_config)?)?;
                if let Some(error) = answer["error"].as_str().filter(|error| !error.is_empty()) {
                    let error = error.to_owned();
                    let _ = engine.request(&configuration(config)?);
                    return Err(error);
                }
                replay
                    .lock()
                    .map_err(|_| "Replay unavailable")?
                    .set_duration(Duration::from_secs(new_config.buffer_secs as u64));
                *config = new_config;
                Ok(IpcResponse::Ok)
            }
            IpcRequest::Shutdown => {
                stop.store(true, Ordering::Release);
                Ok(IpcResponse::Ok)
            }
            IpcRequest::SetTrayVisible(visible) => {
                engine.request(&json!({"action":"tray", "visible": visible}))?;
                Ok(IpcResponse::Ok)
            },
            IpcRequest::ListAudioDevices => {
                let answer = engine.request(&json!({"action":"audio_devices"}))?;
                let devices =
                    serde_json::from_value(answer["devices"].clone()).map_err(|e| e.to_string())?;
                Ok(IpcResponse::AudioDevices(devices))
            }
            IpcRequest::Native(command) => native_data(engine, command),
        }
    })()
    .unwrap_or_else(IpcResponse::Err);
    let _ = control.reply.send(response);
}

fn run() -> Result<(), String> {
    let runtime = encore_common::platform::native_runtime()
        .ok_or("Native capture runtime is missing; reinstall the platform build")?;
    let name = if cfg!(target_os = "macos") {
        "libencore_capture.dylib"
    } else {
        "libencore_capture.so"
    };
    let api = ffi::Api::load(&runtime.join("lib").join(name))?;
    let mut config = Config::load();
    let replay = Arc::new(Mutex::new(RingBuffer::new(
        Duration::from_secs(config.buffer_secs as u64),
        REPLAY_BUDGET,
    )));
    let status = Arc::new(Mutex::new(RecorderStatus {
        state: RecorderState::Idle,
        target: None,
        mode: config.mode,
        fps: config.fps,
        height: config.height,
        buffer_secs: config.buffer_secs,
        hotkey: config.hotkey.clone(),
        last_clip: None,
        error: None,
        audio_warnings: Default::default(),
    }));
    let server = Arc::new(ipc::LocalServer::bind(ipc::PIPE_NAME)?);
    let mut engine = ffi::Engine::open(api.clone(), &runtime, replay.clone())?;
    let answer = engine.request(&configuration(&config)?)?;
    if let Some(error) = answer["error"].as_str().filter(|error| !error.is_empty()) {
        return Err(error.into());
    }
    let saves = Saves::new(api, status.clone());
    let (sender, receiver): (SyncSender<Control>, Receiver<Control>) = mpsc::sync_channel(8);
    let stop = Arc::new(AtomicBool::new(false));
    let listener = thread::spawn({
        let status = status.clone();
        let stop = stop.clone();
        let server = server.clone();
        move || serve(server, status, sender, stop)
    });
    let mut next_status = Instant::now();
    let mut previous_engine_error = String::new();
    while !stop.load(Ordering::Acquire) {
        while let Ok(control) = receiver.try_recv() {
            handle_control(
                control,
                &mut engine,
                &replay,
                &saves,
                &mut config,
                &status,
                &stop,
            );
        }
        engine.tick();
        if Instant::now() >= next_status {
            next_status = Instant::now() + Duration::from_millis(100);
            match engine.request(&json!({"action":"status"})) {
                Ok(native) => {
                    if native["quit_requested"].as_bool().unwrap_or(false) {
                        let _ = ipc::send_ui_command("quit");
                        stop.store(true, Ordering::Release);
                    }
                    if native["open_library"].as_bool().unwrap_or(false) {
                        if let Err(error) = open_library() { status.lock().unwrap().error = Some(error); }
                    }
                    if native["open_folder"].as_bool().unwrap_or(false) {
                        let _ = std::fs::create_dir_all(&config.clips_dir);
                        if let Err(error) = encore_common::platform::reveal_path(&config.clips_dir) {
                            status.lock().unwrap().error = Some(error);
                        }
                    }
                    let target = native["target"].as_str().unwrap_or_default();
                    if native["save_requested"].as_bool().unwrap_or(false) {
                        if let Err(error) = saves.submit(&engine, &replay, &config, target) {
                            status.lock().unwrap().error = Some(error);
                        }
                    }
                    let mut status = status.lock().unwrap();
                    status.state = if saves.pending.load(Ordering::Acquire) != 0 {
                        RecorderState::Saving
                    } else if native["recording"].as_bool().unwrap_or(false) {
                        RecorderState::Recording
                    } else {
                        RecorderState::Idle
                    };
                    status.target = if target.is_empty() {
                        None
                    } else {
                        Some(target.into())
                    };
                    status.mode = config.mode;
                    status.fps = config.fps;
                    status.height = config.height;
                    status.buffer_secs = config.buffer_secs;
                    status.hotkey = config.hotkey.clone();
                    let mut error = native["error"].as_str().unwrap_or_default();
                    if error.is_empty() { error = native["hotkey_warning"].as_str().unwrap_or_default(); }
                    if error != previous_engine_error {
                        status.error = if error.is_empty() {
                            None
                        } else {
                            Some(error.into())
                        };
                        previous_engine_error = error.into();
                    }
                    for i in 0..3 {
                        let warning = native[format!("audio_warning_{i}")]
                            .as_str()
                            .unwrap_or_default();
                        status.audio_warnings[i] = if warning.is_empty() {
                            None
                        } else {
                            Some(warning.into())
                        };
                    }
                }
                Err(error) => status.lock().unwrap().error = Some(error),
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    // Drain accepted exports before releasing the IPC singleton or native library.
    drop(engine);
    drop(saves);
    let _ = listener.join();
    drop(server);
    Ok(())
}

fn open_library() -> Result<(), String> {
    if ipc::send_ui_command("show").unwrap_or(false) { return Ok(()); }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    for ancestor in executable.ancestors() {
        if ancestor.extension().is_some_and(|extension| extension == "app")
            && ancestor.file_name().is_some_and(|name| name != "Encore Recorder.app") {
            Command::new("open").arg("-a").arg(ancestor).spawn().map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    for ancestor in executable.ancestors().skip(1) {
        for relative in ["encore-ui", "bin/encore-ui"] {
            let candidate = ancestor.join(relative);
            if candidate.is_file() {
                Command::new(candidate).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
                    .spawn().map_err(|e| e.to_string())?;
                return Ok(());
            }
        }
    }
    Err("Encore library executable is missing; reinstall the application".into())
}

pub fn main() {
    let arguments: Vec<_> = std::env::args().collect();
    let result = if arguments.get(1).map(String::as_str) == Some("--save") {
        match ipc::send_request(&IpcRequest::SaveClip) {
            Ok(IpcResponse::Ok) => Ok(()),
            Ok(IpcResponse::Err(error)) | Err(error) => Err(error),
            _ => Err("Unexpected recorder response".into()),
        }
    } else if arguments.get(1).map(String::as_str) == Some("--query") {
        match arguments.get(2).map(String::as_str) {
            Some(action @ ("applications" | "audio_applications" | "audio_devices" | "diagnostics")) => query(action),
            _ => Err("Usage: encore-recorder --query applications|audio_applications|audio_devices|diagnostics".into()),
        }
    } else if arguments.get(1).map(String::as_str) == Some("--self-test") {
        match arguments.get(2) {
            Some(directory) => self_test(PathBuf::from(directory)),
            None => Err("Usage: encore-recorder --self-test OUTPUT_DIRECTORY".into()),
        }
    } else {
        run()
    };
    if let Err(error) = result {
        eprintln!("Encore recorder: {error}");
        std::process::exit(1);
    }
}

fn query(action: &str) -> Result<(), String> {
    let runtime = encore_common::platform::native_runtime().ok_or("Native capture runtime missing")?;
    let name = if cfg!(target_os = "macos") { "libencore_capture.dylib" } else { "libencore_capture.so" };
    let api = ffi::Api::load(&runtime.join("lib").join(name))?;
    let replay = Arc::new(Mutex::new(RingBuffer::new(Duration::from_secs(1), 1024 * 1024)));
    let mut engine = ffi::Engine::open(api, &runtime, replay)?;
    println!("{}", engine.request(&json!({"action":action}))?);
    Ok(())
}

/// Exercise real GPU rendering, H.264/AAC encoders, RAM replay and muxing without
/// capturing the screen, microphone, or user applications.
fn self_test(directory: PathBuf) -> Result<(), String> {
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let directory = std::fs::canonicalize(directory).map_err(|e| e.to_string())?;
    let runtime =
        encore_common::platform::native_runtime().ok_or("Native capture runtime missing")?;
    let name = if cfg!(target_os = "macos") {
        "libencore_capture.dylib"
    } else {
        "libencore_capture.so"
    };
    let api = ffi::Api::load(&runtime.join("lib").join(name))?;
    let replay = Arc::new(Mutex::new(RingBuffer::new(
        Duration::from_secs(3),
        8 * 1024 * 1024,
    )));
    let mut engine = ffi::Engine::open(api.clone(), &runtime, replay.clone())?;
    let config = Config {
        fps: 30,
        height: 360,
        buffer_secs: 3,
        clips_dir: directory.clone(),
        ..Config::default()
    };
    let mut request = configuration(&config)?;
    request["config"]["diagnostic_source"] = json!(true);
    request["config"]["state_dir"] = json!(directory);
    let answer = engine.request(&request)?;
    if let Some(error) = answer["error"].as_str().filter(|error| !error.is_empty()) {
        return Err(error.into());
    }
    let start = Instant::now();
    let mut last_status = Value::Null;
    while start.elapsed() < Duration::from_secs(5) {
        engine.tick();
        last_status = engine.request(&json!({"action":"status"}))?;
        thread::sleep(Duration::from_millis(10));
    }
    if !last_status["recording"].as_bool().unwrap_or(false) {
        return Err(format!("Diagnostic encoder did not start: {last_status}"));
    }
    let formats = engine.formats()?;
    let packets = replay
        .lock()
        .map_err(|_| "Replay unavailable")?
        .snapshot(Duration::from_secs(3))
        .ok_or_else(|| format!("Diagnostic A/V replay is incomplete: {last_status}"))?;
    let live_bytes = replay.lock().unwrap().live_bytes();
    if live_bytes > 8 * 1024 * 1024 {
        return Err("Replay exceeded its diagnostic memory budget".into());
    }
    let output = directory.join("encore-self-test.mp4");
    let worker = thread::spawn({
        let api = api.clone();
        move || {
            save_clip(
                &api,
                SaveJob {
                    packets,
                    formats,
                    path: output,
                    game: "Encore diagnostics".into(),
                },
            )
        }
    });
    // Continue pumping the capture loop while the accepted save runs.
    while !worker.is_finished() {
        engine.tick();
        thread::sleep(Duration::from_millis(10));
    }
    worker
        .join()
        .map_err(|_| "Diagnostic save worker panicked")??;
    let report = json!({"ok":true,"encoder":last_status["encoder"],"memory_bytes":live_bytes,"streams":4,"clip":directory.join("encore-self-test.mp4")});
    std::fs::write(
        directory.join("encore-self-test.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("{report}");
    Ok(())
}
