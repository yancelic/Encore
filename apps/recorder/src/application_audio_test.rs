//! Opt-in audio integration: generated tones from owned processes only. No mic/device capture.
use crate::audio::{self, AudioChunk, AudioSource};
use encore_common::ipc::{RecorderState, RecorderStatus};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const FIXTURE: &str = "windows_runtime::application_audio_test::application_audio_fixture";

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
}

fn fixture(exe: &Path, frequency: u32, seconds: u64, tree: bool) -> OwnedChild {
    OwnedChild(Command::new(exe).args(["--ignored", "--exact", FIXTURE, "--nocapture"])
        .env("ENCORE_AUDIO_FIXTURE_HZ", frequency.to_string())
        .env("ENCORE_AUDIO_FIXTURE_SECS", seconds.to_string())
        .env("ENCORE_AUDIO_FIXTURE_TREE", if tree { "1" } else { "0" })
        .creation_flags(0x0800_0000).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().unwrap())
}

#[test]
#[ignore = "helper invoked only by the application-audio integration test; plays a short quiet generated tone"]
fn application_audio_fixture() {
    let Ok(frequency) = std::env::var("ENCORE_AUDIO_FIXTURE_HZ") else { return };
    let frequency: u32 = frequency.parse().unwrap();
    let seconds: u64 = std::env::var("ENCORE_AUDIO_FIXTURE_SECS").unwrap().parse().unwrap();
    assert!(seconds <= 15);
    if std::env::var("ENCORE_AUDIO_FIXTURE_TREE").as_deref() == Ok("1") {
        let mut child = fixture(&std::env::current_exe().unwrap(), frequency, seconds, false);
        assert!(child.0.wait().unwrap().success());
        return;
    }
    wasapi::initialize_mta().ok().unwrap();
    let device = wasapi::DeviceEnumerator::new().unwrap().get_default_device(wasapi::Direction::Render).unwrap();
    let mut client = device.get_iaudioclient().unwrap();
    let format = wasapi::WaveFormat::new(16, 16, wasapi::SampleType::Int, 48_000, 2, None);
    client.initialize_client(&format, wasapi::Direction::Render,
        wasapi::StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 200_000 }, None).unwrap();
    let event = client.set_get_eventhandle().unwrap();
    let render = client.get_audiorenderclient().unwrap();
    let mut position = 0;
    let mut fill = || {
        let frames = client.get_available_space_in_frames().unwrap() as usize;
        if frames == 0 { return; }
        let mut pcm = Vec::with_capacity(frames * 4);
        for _ in 0..frames {
            let sample = (1000.0 * (std::f64::consts::TAU * frequency as f64 * position as f64 / 48_000.0).sin()) as i16;
            pcm.extend_from_slice(&sample.to_le_bytes());
            pcm.extend_from_slice(&sample.to_le_bytes());
            position += 1;
        }
        render.write_to_device(&pcm, None).unwrap();
    };
    fill();
    client.start_stream().unwrap();
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        let _ = event.wait_for_event(20);
        fill();
    }
    client.stop_stream().unwrap();
}

struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() { thread.join().unwrap(); }
    }
}

fn amplitude(pcm: &[u8], hz: f64) -> f64 {
    let count = pcm.len() / 4;
    let (mut re, mut im) = (0.0, 0.0);
    for (i, frame) in pcm.chunks_exact(4).enumerate() {
        let sample = i16::from_le_bytes([frame[0], frame[1]]) as f64;
        let phase = std::f64::consts::TAU * hz * i as f64 / 48_000.0;
        re += sample * phase.cos();
        im += sample * phase.sin();
    }
    2.0 * re.hypot(im) / count.max(1) as f64
}

fn collect(rx: &mpsc::Receiver<AudioChunk>, seconds: u64) -> Vec<u8> {
    let end = Instant::now() + Duration::from_secs(seconds);
    let t0 = crate::clock::qpc_100ns();
    let mut timeline = audio::AudioTimeline::new(t0);
    let mut written = 0;
    let mut pcm = Vec::new();
    while Instant::now() < end {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(5)) { timeline.push(chunk); }
        while let Ok(chunk) = rx.try_recv() { timeline.push(chunk); }
        // Use the recorder's actual synchronization allowance, so the test also
        // detects packets arriving too late to survive its audio timeline.
        let elapsed = crate::clock::qpc_100ns() - t0 - crate::recorder::AUDIO_LOOKBACK_100NS;
        let due = (elapsed.max(0) as i128 * 48_000 / 10_000_000) as usize;
        if due > written { pcm.extend(timeline.pop_samples(due - written)); written = due; }
    }
    pcm
}

#[test]
#[ignore = "requires audio-service permission; plays generated tones and captures only its own fixture process tree"]
fn application_audio_filters_tree_and_reconnects() {
    assert!(encore_common::processes::application_loopback_supported());
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("encore_audio_fixture_{}_{}", std::process::id(), stamp));
    std::fs::create_dir_all(&root).unwrap();
    let app_name = format!("EncoreChatFixture_{}.exe", std::process::id());
    let app = root.join(&app_name);
    let other = root.join("EncoreOtherFixture.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &app).unwrap();
    std::fs::copy(&app, &other).unwrap();
    let status = Arc::new(Mutex::new(RecorderStatus {
        state: RecorderState::Recording, target: None, mode: encore_common::CaptureMode::GameDetection,
        fps: 60, height: 720, buffer_secs: 2, hotkey: "Alt+S".into(), last_clip: None, error: None,
        audio_warnings: Default::default(),
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel(32);
    let capture = Capture { stop: stop.clone(), thread: Some(std::thread::spawn({
        let status = status.clone();
        move || audio::run_application(AudioSource::Application(app_name), 1, tx, stop, status)
    })) };
    std::thread::sleep(Duration::from_millis(100));
    assert!(status.lock().unwrap().audio_warnings[1].as_deref().is_some_and(|s| s.contains("not running")));
    // Parent is silent; only its child renders 440 Hz. A separate tree renders 660 Hz.
    let mut unrelated = fixture(&other, 660, 12, false);
    let mut first = fixture(&app, 440, 4, true);
    // Exercise the game's fixed-PID source concurrently with the chat resolver,
    // using the same output device, and prove the excluded renderer is active.
    let game_stop = Arc::new(AtomicBool::new(false));
    let (game_tx, game_rx) = mpsc::sync_channel(32);
    let game_capture = Capture { stop: game_stop.clone(), thread: Some(std::thread::spawn({
        let status = status.clone();
        let pid = unrelated.0.id();
        move || audio::run_application(AudioSource::Process(pid), 0, game_tx, game_stop, status)
    })) };
    let game_samples = std::thread::spawn(move || collect(&game_rx, 3));
    let first_pcm = collect(&rx, 3);
    let game_pcm = game_samples.join().unwrap();
    drop(game_capture);
    let game_tone = amplitude(&game_pcm, 660.0);
    assert!(game_tone > 100.0, "game PID source did not capture its tone: {game_tone}");
    assert!(amplitude(&game_pcm, 440.0) < game_tone * 0.05, "chat audio leaked into the game track");
    let wanted = amplitude(&first_pcm, 440.0);
    let excluded = amplitude(&first_pcm, 660.0);
    eprintln!("first process tree: 440 Hz={wanted:.1}, excluded 660 Hz={excluded:.1}");
    assert!(wanted > 100.0, "child-process tone not captured: {:?}", status.lock().unwrap().audio_warnings);
    assert!(excluded < wanted * 0.05, "unrelated application's audio leaked into the chat track");
    assert!(first.0.wait().unwrap().success());
    // Drain all old packets while allowing the one-second resolver to see process exit.
    let _ = collect(&rx, 2);
    assert!(status.lock().unwrap().audio_warnings[1].as_deref().is_some_and(|s| s.contains("not running")));
    let mut second = fixture(&app, 880, 4, true);
    let second_pcm = collect(&rx, 3);
    let restarted = amplitude(&second_pcm, 880.0);
    let excluded = amplitude(&second_pcm, 660.0);
    eprintln!("restarted process tree: 880 Hz={restarted:.1}, excluded 660 Hz={excluded:.1}");
    assert!(restarted > 100.0, "did not reconnect after application restart");
    assert!(excluded < restarted * 0.05);
    assert!(status.lock().unwrap().audio_warnings[1].is_none());
    drop(capture);
    assert!(second.0.wait().unwrap().success());
    unrelated.0.kill().unwrap();
    let _ = unrelated.0.wait();
    drop((first, second, unrelated));
    std::fs::remove_dir_all(root).unwrap();
}
