//! Rate-limited WGC capture + continuous Media Foundation encoding + bounded RAM replay.
#[cfg(test)]
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use encore_common::ipc::{RecorderState, RecorderStatus};
use encore_common::{AudioMode, Config};
use encore_encode::RingBuffer;
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
#[cfg(test)]
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsIconic, IsWindow};
use windows::core::{IInspectable, IUnknown, Interface};

use crate::audio::{self, AudioChunk, AudioTimeline};
use crate::clock::{self, HighResTimer};
use crate::diagnostics::{CaptureCounters, ReplayCounters, Stats};
use crate::encoder::{self, Err, MfGpu, ReplayEncoder};
use crate::save_worker::SaveWorker;
#[cfg(test)]
use crate::{clip, encoder::SampleWriter};

pub(crate) const AUDIO_LOOKBACK_100NS: i64 = 250_000; // 25 ms behind real time so WASAPI packets have arrived
const INITIAL_GPU_SLOTS: usize = 6;
const MAX_GPU_SLOTS: usize = 24;
const REPLAY_MAX_BYTES: usize = 512 * 1024 * 1024;

/// One copy per recording-time bucket, even on Windows versions without MinUpdateInterval.
/// Fixed buckets tolerate delivery jitter without accidentally halving the recording rate.
struct FrameLimiter {
    fps: u32,
    origin: Option<i64>,
    last_bucket: i128,
}

impl FrameLimiter {
    fn new(fps: u32) -> Self { Self { fps: fps.clamp(1, 240), origin: None, last_bucket: -1 } }

    fn allow(&mut self, timestamp: i64) -> bool {
        let origin = *self.origin.get_or_insert(timestamp);
        if timestamp < origin {
            self.origin = Some(timestamp);
            self.last_bucket = -1;
        }
        let elapsed = timestamp as i128 - self.origin.unwrap() as i128;
        // Center buckets around nominal capture times so small timestamp jitter does not
        // move every other 60 Hz frame into the preceding bucket. Startup can admit one
        // extra frame; there is no accumulating rate overshoot.
        let bucket = (elapsed * self.fps as i128 + 5_000_000) / 10_000_000;
        if bucket <= self.last_bucket { return false; }
        self.last_bucket = bucket;
        true
    }
}

/// What to capture without holding WinRT types across threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTarget {
    #[cfg(test)]
    Monitor(usize),
    Window(usize),
}

/// Handle to stop a running capture session cleanly.
pub struct CaptureControl {
    stop: Arc<AtomicBool>,
    notify: Arc<Condvar>,
    state: Arc<Mutex<SharedFrames>>,
    thread: Option<JoinHandle<()>>,
}

impl CaptureControl {
    pub fn is_finished(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || self.thread.as_ref().is_none_or(|t| t.is_finished())
    }

    pub fn stop(mut self) -> Result<(), Err> {
        self.stop.store(true, Ordering::Relaxed);
        {
            let _g = self.state.lock().unwrap();
            self.notify.notify_all();
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        Ok(())
    }
}

impl Drop for CaptureControl {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        {
            let _g = self.state.lock().unwrap();
            self.notify.notify_all();
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct SharedFrames {
    dims: (u32, u32),
    slots: Vec<ID3D11Texture2D>,
    latest: Option<usize>,
    enc_in_use: Option<usize>,
    seq: u64,
    first_frame_qpc: Option<i64>,
    latest_qpc: i64,
    encoded_seq: u64,
    limiter: FrameLimiter,
    counters: CaptureCounters,
}

impl SharedFrames {
    fn new(dims: (u32, u32), fps: u32) -> Self {
        Self {
            dims,
            slots: Vec::new(),
            latest: None,
            enc_in_use: None,
            seq: 0,
            first_frame_qpc: None,
            latest_qpc: 0,
            encoded_seq: 0,
            limiter: FrameLimiter::new(fps),
            counters: CaptureCounters::default(),
        }
    }
}

fn create_gpu_texture(gpu: &MfGpu, w: u32, h: u32) -> Result<ID3D11Texture2D, Err> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut tex = None;
    unsafe { gpu.device.CreateTexture2D(&desc, None, Some(&mut tex))? };
    Ok(tex.ok_or("CreateTexture2D returned None")?)
}

fn create_gpu_slots(gpu: &MfGpu, w: u32, h: u32) -> Result<Vec<ID3D11Texture2D>, Err> {
    (0..INITIAL_GPU_SLOTS).map(|_| create_gpu_texture(gpu, w, h)).collect()
}

/// COM refcount of `tex` (1 when only `SharedFrames` holds it; >1 while MF holds an `IMFSample`).
fn com_ref_count(tex: &ID3D11Texture2D) -> u32 {
    let unk: IUnknown = tex.cast().expect("IUnknown");
    unsafe {
        let _ = (Interface::vtable(&unk).AddRef)(unk.as_raw());
        // `cast` itself owns one extra reference until `unk` drops on return.
        (Interface::vtable(&unk).Release)(unk.as_raw()).saturating_sub(1)
    }
}

fn free_gpu_slot(st: &SharedFrames) -> Option<usize> {
    (0..st.slots.len()).find(|&i| {
        Some(i) != st.latest && Some(i) != st.enc_in_use && com_ref_count(&st.slots[i]) == 1
    })
}

fn create_capture_item(target: CaptureTarget) -> Result<GraphicsCaptureItem, Err> {
    let interop: IGraphicsCaptureItemInterop =
        windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let item = match target {
        #[cfg(test)]
        CaptureTarget::Monitor(h) => unsafe { interop.CreateForMonitor(HMONITOR(h as *mut _))? },
        CaptureTarget::Window(h) => unsafe { interop.CreateForWindow(HWND(h as *mut _))? },
    };
    Ok(item)
}

fn target_process_id(target: CaptureTarget) -> Option<u32> {
    match target {
        #[cfg(test)]
        CaptureTarget::Monitor(_) => None,
        CaptureTarget::Window(handle) => {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(HWND(handle as *mut _), Some(&mut pid)); }
            (pid != 0).then_some(pid)
        }
    }
}

fn target_is_available(target: CaptureTarget, owner_pid: Option<u32>) -> bool {
    match target {
        #[cfg(test)]
        CaptureTarget::Monitor(_) => true,
        CaptureTarget::Window(handle) => {
            let hwnd = HWND(handle as *mut _);
            (unsafe { IsWindow(Some(hwnd)).as_bool() && !IsIconic(hwnd).as_bool() })
                && owner_pid.is_some() && target_process_id(target) == owner_pid
        }
    }
}

fn open_replay_encoder(
    gpu: &MfGpu, replay: Arc<Mutex<RingBuffer>>, input: (u32, u32), height: u32, fps: u32,
) -> Result<ReplayEncoder, Err> {
    let output = encore_common::scale_dimensions(input.0, input.1, height);
    ReplayEncoder::open(gpu, replay, input, output, fps, encore_common::target_bitrate(output.0, output.1, fps))
}

fn spawn_audio_track(
    source: Option<audio::AudioSource>,
    track: usize,
    label: &'static str,
    stop: &Arc<AtomicBool>,
    status: &Arc<Mutex<RecorderStatus>>,
) -> (Option<Receiver<AudioChunk>>, Option<JoinHandle<()>>) {
    let Some(source) = source else { return (None, None) };
    let (tx, rx) = mpsc::sync_channel(32);
    let stop_c = stop.clone();
    let status = status.clone();
    let h = thread::spawn(move || match source {
        audio::AudioSource::Device(kind, want) => {
            if let Err(e) = audio::run(kind, label, tx, want, stop_c) {
                audio::warning(&status, track, Some(format!("{label} audio device is unavailable; this track is silent. Check Settings. ({e})")));
            }
        }
        application => audio::run_application(application, track, tx, stop_c, status),
    });
    (Some(rx), Some(h))
}

#[allow(clippy::too_many_arguments)]
fn run_session(
    target: CaptureTarget,
    game_name: Option<String>,
    cfg: Config,
    save: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<SharedFrames>>,
    notify: Arc<Condvar>,
    status: Arc<Mutex<RecorderStatus>>,
) {
    encoder::ensure_com_mta();
    let owner_pid = target_process_id(target);
    if !target_is_available(target, owner_pid) { return; }
    let gpu = match MfGpu::new() {
        Ok(g) => Arc::new(g),
        Err(e) => {
            eprintln!("GPU/MF init failed: {e}");
            return;
        }
    };
    let item = match create_capture_item(target) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Could not create WGC capture item: {e}");
            return;
        }
    };
    let init_size = match item.Size() {
        Ok(s) if s.Width >= 2 && s.Height >= 2 => s,
        Ok(_) => {
            eprintln!("Capture target has zero size");
            return;
        }
        Err(e) => {
            eprintln!("Could not query capture item size: {e}");
            return;
        }
    };
    let init_dims = ((init_size.Width as u32) & !1, (init_size.Height as u32) & !1);
    {
        let mut st = shared.lock().unwrap();
        st.dims = init_dims;
    }

    let fps = cfg.fps.clamp(1, 240);
    let replay = Arc::new(Mutex::new(RingBuffer::new(std::time::Duration::from_secs(cfg.buffer_secs.max(1) as u64), REPLAY_MAX_BYTES)));
    let mut active = match open_replay_encoder(&gpu, replay.clone(), init_dims, cfg.height, fps) {
        Ok(writer) => Some(writer),
        Err(error) => { eprintln!("RAM replay encoder initialization failed: {error}"); return; }
    };
    let saver = SaveWorker::new(gpu.clone(), game_name, status.clone());
    let mut active_dims = init_dims;

    // Spawn up to 3 audio capture threads: Game (0), Chat (1), Mic (2).
    let audio_stop = Arc::new(AtomicBool::new(false));
    let mut audio_rxs: [Option<Receiver<AudioChunk>>; 3] = [None, None, None];
    let mut audio_threads: Vec<JoinHandle<()>> = Vec::new();

    status.lock().unwrap().audio_warnings = Default::default();
    let enabled = |s: &Option<String>| s.as_deref().is_some_and(|s| !s.is_empty() && !s.eq_ignore_ascii_case("off"));
    let sources = match cfg.audio_mode {
        AudioMode::Devices => [
            (!cfg.audio_device.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("off")))
                .then(|| audio::AudioSource::Device(audio::AudioSourceKind::Loopback, cfg.audio_device.clone())),
            enabled(&cfg.chat_device).then(|| audio::AudioSource::Device(audio::AudioSourceKind::Loopback, cfg.chat_device.clone())),
        ],
        AudioMode::Applications => [
            owner_pid.filter(|_| cfg.game_audio_enabled).map(audio::AudioSource::Process),
            cfg.chat_application.as_deref().filter(|exe| encore_common::processes::valid_executable(exe))
                .map(|exe| audio::AudioSource::Application(exe.to_string())),
        ],
    };
    let mic = enabled(&cfg.mic_device).then(|| audio::AudioSource::Device(audio::AudioSourceKind::Capture, cfg.mic_device.clone()));
    for (track, (source, label)) in sources.into_iter().chain([mic]).zip(["Game", "Chat", "Microphone"]).enumerate() {
        let (rx, thread) = spawn_audio_track(source, track, label, &audio_stop, &status);
        audio_rxs[track] = rx;
        if let Some(thread) = thread { audio_threads.push(thread); }
    }

    // Bound capture delivery to the recording rate where Windows supports it.
    let pool = match Direct3D11CaptureFramePool::CreateFreeThreaded(
        &gpu.winrt_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        3,
        init_size,
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("CreateFreeThreaded failed: {e}");
            audio_stop.store(true, Ordering::Relaxed);
            return;
        }
    };
    let session = match pool.CreateCaptureSession(&item) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("CreateCaptureSession failed: {e}");
            let _ = pool.Close();
            audio_stop.store(true, Ordering::Relaxed);
            return;
        }
    };
    let _ = session.SetIsBorderRequired(false);
    match session.SetMinUpdateInterval(TimeSpan { Duration: 10_000_000 / fps as i64 }) {
        Ok(()) => println!("WGC capture interval set to {fps} fps; software copy limiter enabled."),
        Err(e) => println!("WGC interval unavailable ({e}); using software copy limiter at {fps} fps."),
    }

    let closed_tok = item
        .Closed(&TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new({
            let stop = stop.clone();
            let shared = shared.clone();
            let notify = notify.clone();
            move |_, _| {
                stop.store(true, Ordering::Relaxed);
                let _g = shared.lock().unwrap();
                notify.notify_all();
                Ok(())
            }
        }))
        .ok();

    let pool_w = Arc::new(AtomicI32::new(init_size.Width));
    let pool_h = Arc::new(AtomicI32::new(init_size.Height));
    // Do not let overlapping event callbacks reserve/copy the same slot or race a resize.
    let callback_gate = Arc::new(Mutex::new(()));
    let frame_tok = pool.FrameArrived(&TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new({
        let gpu = gpu.clone();
        let shared = shared.clone();
        let notify = notify.clone();
        let pool_ref = pool.clone();
        let stop = stop.clone();
        let pool_w = pool_w.clone();
        let pool_h = pool_h.clone();
        let callback_gate = callback_gate.clone();
        move |p_arg, _| {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let Ok(_callback) = callback_gate.try_lock() else { return Ok(()) };
            let cb_t0 = Instant::now();
            let Some(p) = p_arg.as_ref() else { return Ok(()) };
            // Drain to newest available frame if more than one queued.
            let mut latest_frame = None;
            let mut received = 0u64;
            while let Ok(f) = p.TryGetNextFrame() {
                received += 1;
                if let Some(old) = latest_frame.replace(f) {
                    let _ = old.Close();
                }
            }
            let Some(f) = latest_frame else { return Ok(()) };
            {
                let mut st = shared.lock().unwrap();
                st.counters.received += received;
                st.counters.drained += received.saturating_sub(1);
            }

            let csize = f.ContentSize()?;
            if csize.Width != pool_w.load(Ordering::Relaxed) || csize.Height != pool_h.load(Ordering::Relaxed) {
                let _ = f.Close();
                if csize.Width >= 2 && csize.Height >= 2 {
                    pool_ref.Recreate(
                        &gpu.winrt_device,
                        DirectXPixelFormat::B8G8R8A8UIntNormalized,
                        3,
                        csize,
                    )?;
                    pool_w.store(csize.Width, Ordering::Relaxed);
                    pool_h.store(csize.Height, Ordering::Relaxed);
                }
                return Ok(());
            }

            let even = ((csize.Width as u32) & !1, (csize.Height as u32) & !1);
            if even.0 == 0 || even.1 == 0 {
                let _ = f.Close();
                return Ok(());
            }

            let now_qpc = clock::qpc_100ns();
            let frame_qpc = f
                .SystemRelativeTime()
                .ok()
                .map(|t| t.Duration)
                .filter(|&t| (now_qpc - t).abs() < 5_000_000)
                .unwrap_or(now_qpc);

            let (slot, dst_tex) = {
                let mut st = shared.lock().unwrap();
                if st.dims != even || st.slots.is_empty() {
                    if let Ok(new_slots) = create_gpu_slots(&gpu, even.0, even.1) {
                        st.slots = new_slots;
                        st.dims = even;
                        st.latest = None;
                        st.enc_in_use = None;
                        st.limiter = FrameLimiter::new(fps);
                    } else {
                        drop(st);
                        let _ = f.Close();
                        return Ok(());
                    }
                }
                if !st.limiter.allow(frame_qpc) {
                    st.counters.throttled += 1;
                    drop(st);
                    let _ = f.Close();
                    return Ok(());
                }
                let slot = match free_gpu_slot(&st) {
                    Some(i) => i,
                    None if st.slots.len() < MAX_GPU_SLOTS => {
                        if let Ok(extra) = create_gpu_texture(&gpu, even.0, even.1) {
                            st.slots.push(extra);
                            st.slots.len() - 1
                        } else {
                            st.counters.busy_drops += 1;
                            drop(st);
                            let _ = f.Close();
                            return Ok(());
                        }
                    }
                    None => {
                        st.counters.busy_drops += 1;
                        drop(st);
                        let _ = f.Close();
                        return Ok(());
                    }
                };
                let dst = st.slots[slot].clone();
                (slot, dst)
            };

            // Rate-limited frames never reach texture extraction or GPU copy submission.
            let surf = f.Surface()?;
            let acc: IDirect3DDxgiInterfaceAccess = surf.cast()?;
            let src_tex: ID3D11Texture2D = unsafe { acc.GetInterface()? };
            let mut src_desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { src_tex.GetDesc(&mut src_desc) };
            let cw = even.0.min(src_desc.Width);
            let ch = even.1.min(src_desc.Height);
            if cw == 0 || ch == 0 {
                let _ = f.Close();
                return Ok(());
            }
            let copy_box = D3D11_BOX { left: 0, top: 0, front: 0, right: cw, bottom: ch, back: 1 };
            unsafe {
                gpu.context.CopySubresourceRegion(&dst_tex, 0, 0, 0, 0, &src_tex, 0, Some(&copy_box));
                gpu.context.Flush();
            }
            let _ = f.Close();

            let cb_ns = cb_t0.elapsed().as_nanos() as u64;
            {
                let mut st = shared.lock().unwrap();
                if st.dims == even {
                    if st.seq > st.encoded_seq {
                        st.counters.superseded += 1;
                    }
                    st.latest = Some(slot);
                    st.seq += 1;
                    st.latest_qpc = frame_qpc;
                    st.counters.copied += 1;
                    if st.first_frame_qpc.is_none() {
                        st.first_frame_qpc = Some(frame_qpc);
                    }
                    st.counters.copy_callback_ns += cb_ns;
                    st.counters.copy_callback_max_ns = st.counters.copy_callback_max_ns.max(cb_ns);
                }
                notify.notify_one();
            }
            Ok(())
        }
    }));

    if let Err(e) = &frame_tok {
        eprintln!("FrameArrived registration failed: {e}");
        if let Some(tok) = closed_tok { let _ = item.RemoveClosed(tok); }
        let _ = session.Close();
        let _ = pool.Close();
        audio_stop.store(true, Ordering::Relaxed);
        for handle in audio_threads { let _ = handle.join(); }
        return;
    }

    if let Err(e) = session.StartCapture() {
        eprintln!("StartCapture failed: {e}");
        if let Ok(tok) = frame_tok {
            let _ = pool.RemoveFrameArrived(tok);
        }
        if let Some(tok) = closed_tok {
            let _ = item.RemoveClosed(tok);
        }
        let _ = session.Close();
        let _ = pool.Close();
        audio_stop.store(true, Ordering::Relaxed);
        for h in audio_threads {
            let _ = h.join();
        }
        return;
    }

    // Wait for Frame 0
    let mut t0_100ns = {
        let mut st = shared.lock().unwrap();
        while st.latest.is_none() && !stop.load(Ordering::Relaxed) {
            st = notify.wait_timeout(st, std::time::Duration::from_millis(250)).unwrap().0;
            if !target_is_available(target, owner_pid) {
                stop.store(true, Ordering::Relaxed);
            }
        }
        st.first_frame_qpc.unwrap_or_else(clock::qpc_100ns)
    };

    if !stop.load(Ordering::Relaxed) {
        let mut timelines = [
            AudioTimeline::new(t0_100ns),
            AudioTimeline::new(t0_100ns),
            AudioTimeline::new(t0_100ns),
        ];
        let timer = HighResTimer::new();
        let mut stats = Stats::from_env();

        let mut global_idx: u64 = 0;
        let mut local_idx: u64 = 0;
        let mut last_seq: u64 = 0;
        let mut next_target_check = Instant::now();

        while !stop.load(Ordering::Relaxed) {
            // Closed events are not delivered reliably on every Windows/driver combination.
            // Poll window lifetime independently, including before the very first frame.
            if Instant::now() >= next_target_check {
                next_target_check = Instant::now() + std::time::Duration::from_millis(250);
                if !target_is_available(target, owner_pid) {
                    stop.store(true, Ordering::Relaxed);
                    break;
                }
            }
            let window_end_100ns = t0_100ns + encoder::ticks_100ns(global_idx + 1, fps);
            let wakeup_100ns = window_end_100ns + AUDIO_LOOKBACK_100NS;
            timer.sleep_until_100ns(wakeup_100ns);
            if stop.load(Ordering::Relaxed) {
                break;
            }

            // Guard against large OS sleep / pause (>500 ms late): skip forward cleanly without a burst.
            let now = clock::qpc_100ns();
            if now - wakeup_100ns >= 10_000_000 / fps as i64 {
                if let Some(stats) = stats.as_mut() { stats.late_tick(); }
            }
            if now - wakeup_100ns > 5_000_000 {
                let behind_frames = (((now - wakeup_100ns) as i128 * fps as i128) / 10_000_000) as i64;
                if behind_frames > 0 {
                    t0_100ns += encoder::ticks_100ns(behind_frames as u64, fps);
                }
            }

            for (track, rx_opt) in audio_rxs.iter().enumerate() {
                if let Some(rx) = rx_opt {
                    while let Ok(chunk) = rx.try_recv() {
                        timelines[track].push(chunk);
                    }
                }
            }

            let (slot, tex, cur_dims, seq, frame_qpc) = {
                let mut st = shared.lock().unwrap();
                let s = match st.latest {
                    Some(s) => s,
                    None => continue,
                };
                st.enc_in_use = Some(s);
                let tex = st.slots[s].clone();
                (s, tex, st.dims, st.seq, st.latest_qpc)
            };

            // Drain the old epoch before clearing history. Pending saves still own their
            // immutable old-size snapshots and remain charged to this same memory budget.
            if cur_dims != active_dims {
                if let Some(writer) = active.take() {
                    if let Err(error) = writer.finish() { eprintln!("draining replay encoder: {error}"); }
                }
                replay.lock().unwrap().clear();
                active_dims = cur_dims;
                local_idx = 0;
                match open_replay_encoder(&gpu, replay.clone(), active_dims, cfg.height, fps) {
                    Ok(writer) => active = Some(writer),
                    Err(e) => {
                        eprintln!("replay encoder restart failed after resize: {e}");
                        shared.lock().unwrap().enc_in_use = None;
                        break;
                    }
                }
            }

            let s0 = encoder::audio_samples_at_frame(local_idx, fps);
            let s1 = encoder::audio_samples_at_frame(local_idx + 1, fps);
            let a_pts = encoder::ticks_100ns(s0, 48_000);
            let a_dur = encoder::ticks_100ns(s1, 48_000) - a_pts;
            let sample_count = (s1 - s0) as usize;
            let pcms: [Vec<u8>; 3] = std::array::from_fn(|t| timelines[t].pop_samples(sample_count));

            if let Some(ref writer) = active {
                let v_pts = encoder::ticks_100ns(local_idx, fps);
                let v_dur = encoder::ticks_100ns(local_idx + 1, fps) - v_pts;
                let submit_start = Instant::now();
                let frame_age = clock::qpc_100ns() - frame_qpc;
                let result = writer.write_video(&tex, v_pts, v_dur);
                if let Some(stats) = stats.as_mut() {
                    stats.video(result.is_ok(), seq == last_seq, submit_start.elapsed().as_nanos(), frame_age);
                }
                if let Err(e) = result {
                    eprintln!("write_video failed: {e}");
                } else {
                    last_seq = seq;
                    let mut st = shared.lock().unwrap();
                    st.encoded_seq = st.encoded_seq.max(seq);
                }
                for (t, pcm) in pcms.iter().enumerate() {
                    if let Err(e) = writer.write_audio(t, pcm, a_pts, a_dur) {
                        eprintln!("write_audio[{t}] failed: {e}");
                        if let Some(stats) = stats.as_mut() { stats.audio_error(); }
                    }
                }
            }

            {
                let mut st = shared.lock().unwrap();
                if st.enc_in_use == Some(slot) {
                    st.enc_in_use = None;
                }
            }

            global_idx += 1;
            local_idx += 1;

            if let Some(stats) = stats.as_mut() {
                if stats.due() {
                    let (counters, slots) = {
                        let mut st = shared.lock().unwrap();
                        (std::mem::take(&mut st.counters), st.slots.len())
                    };
                    let out_dims = encore_common::scale_dimensions(active_dims.0, active_dims.1, cfg.height);
                    let replay_stats = {
                        let buffer = replay.lock().unwrap();
                        ReplayCounters {
                            bytes: buffer.live_bytes(), limit: buffer.max_bytes(), history_ms: buffer.available_duration().as_millis(),
                            memory_drops: buffer.memory_drops(), memory_evictions: buffer.memory_evictions(), pending_saves: saver.pending(),
                        }
                    };
                    stats.report(counters, slots, active_dims, out_dims, fps, replay_stats);
                }
            }

            let want_save = save.swap(false, Ordering::Relaxed);
            if want_save {
                let result = (|| -> Result<(), String> {
                    if saver.pending() >= 2 { return Err("Replay save queue is busy; wait for the current clips to finish".into()); }
                    let writer = active.as_ref().ok_or("Replay encoder is unavailable")?;
                    let snapshot = writer.snapshot(std::time::Duration::from_secs(cfg.buffer_secs.max(1) as u64)).map_err(|e| e.to_string())?;
                    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
                    saver.submit(snapshot, cfg.clips_dir.join(format!("Encore_{stamp}.mp4")))
                })();
                if let Err(error) = result {
                    eprintln!("Replay save request: {error}");
                    status.lock().unwrap().error = Some(error);
                }
            }
        }

        if let Some(writer) = active.take() {
            if let Err(error) = writer.finish() { eprintln!("draining replay encoder: {error}"); }
        }
    }

    if let Ok(tok) = frame_tok {
        let _ = pool.RemoveFrameArrived(tok);
    }
    if let Some(tok) = closed_tok {
        let _ = item.RemoveClosed(tok);
    }
    let _ = session.Close();
    let _ = pool.Close();

    audio_stop.store(true, Ordering::Relaxed);
    for h in audio_threads {
        let _ = h.join();
    }
    drop(saver); // Finish bounded pending saves before Media Foundation shuts down.
}

/// Start capturing `target` (a monitor or a window) in the background.
pub fn start(
    target: CaptureTarget,
    game_name: Option<String>,
    cfg: &Config,
    save: Arc<AtomicBool>,
    status: Arc<Mutex<RecorderStatus>>,
) -> Result<CaptureControl, Err> {
    let stop = Arc::new(AtomicBool::new(false));
    let notify = Arc::new(Condvar::new());
    let shared = Arc::new(Mutex::new(SharedFrames::new((0, 0), cfg.fps)));

    let cfg_c = cfg.clone();
    let stop_c = stop.clone();
    let shared_c = shared.clone();
    let notify_c = notify.clone();
    let thread = thread::spawn(move || {
        run_session(target, game_name, cfg_c, save, stop_c.clone(), shared_c.clone(), notify_c.clone(), status.clone());
        // Also wake the supervisor/consumers when startup or an encoder failed asynchronously.
        if !stop_c.swap(true, Ordering::Relaxed) {
            let mut st = status.lock().unwrap();
            st.state = RecorderState::Idle;
            st.target = None;
            st.error = Some("Capture session ended; waiting to retry the game window.".into());
        }
        let _guard = shared_c.lock().unwrap();
        notify_c.notify_all();
    });

    Ok(CaptureControl { stop, notify, state: shared, thread: Some(thread) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn copy_limiter_handles_high_refresh_and_fractional_intervals() {
        for fps in [30, 60, 120, 144, 240] {
            let mut limiter = FrameLimiter::new(fps);
            let accepted = (0..14_400).filter(|&i| limiter.allow(i * 10_000_000 / 1440)).count();
            assert!((fps as usize * 10..=fps as usize * 10 + 1).contains(&accepted), "copy count {accepted} at {fps} fps");
        }
        let mut limiter = FrameLimiter::new(60);
        // Real delivery jitters around 60 Hz; comparing each delta to 16.667 ms would halve FPS.
        let accepted = (0..600).filter(|&i| {
            let jitter = if i % 2 == 0 { 100 } else { -100 };
            limiter.allow(1_000 + i * 10_000_000 / 60 + jitter)
        }).count();
        assert!(accepted >= 598, "only {accepted} frames survived delivery jitter");
        assert!(limiter.allow(200_000_000));
        assert!(!limiter.allow(200_000_000)); // no catch-up burst
    }

    #[test]
    fn gpu_slots_exclude_encoder_owned_samples_and_recover_after_release() {
        let gpu = MfGpu::new().expect("MfGpu");
        let mut st = SharedFrames::new((64, 64), 60);
        st.slots = create_gpu_slots(&gpu, 64, 64).unwrap();
        assert_eq!(com_ref_count(&st.slots[0]), 1, "checking ownership must not count its own temporary reference");
        st.latest = Some(0);
        st.enc_in_use = Some(1);
        let held: Vec<_> = st.slots.iter().skip(2).map(|tex| unsafe {
            windows::Win32::Media::MediaFoundation::MFCreateDXGISurfaceBuffer(
                &ID3D11Texture2D::IID, tex, 0, false,
            ).unwrap()
        }).collect();
        assert!(free_gpu_slot(&st).is_none(), "never overwrite a surface still owned by an MF buffer");
        drop(held);
        assert_eq!(free_gpu_slot(&st), Some(2));
        assert_eq!(st.slots.len(), INITIAL_GPU_SLOTS);
    }

    #[test]
    #[ignore = "requires Windows capture permission and an interactive desktop; records only the generated test window"]
    fn window_capture_limits_copies_saves_motion_and_stops_when_window_closes() {
        let window = crate::test_window::TestWindow::new();
        encoder::ensure_com_mta();
        let size = create_capture_item(CaptureTarget::Window(window.hwnd)).unwrap().Size().unwrap();
        let expected_dims = ((size.Width as u32) & !1, (size.Height as u32) & !1);
        let root = std::env::temp_dir().join(format!("encore_window_test_{}_{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let cfg = Config {
            fps: 60, height: 0, buffer_secs: 2, clips_dir: root.join("clips"),
            audio_device: Some("off".into()), chat_device: Some("off".into()), mic_device: Some("off".into()),
            ..Config::default()
        };
        let save = Arc::new(AtomicBool::new(false));
        let status = test_status();
        let ctl = start(CaptureTarget::Window(window.hwnd), Some("Controlled test".into()),
            &cfg, save.clone(), status.clone()).unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(8);
        loop {
            if ctl.state.lock().unwrap().seq > 0 { break; }
            assert!(!ctl.is_finished(), "capture stopped during startup: {:?}", status.lock().unwrap().error);
            assert!(Instant::now() < deadline, "no WGC frame arrived");
            thread::sleep(std::time::Duration::from_millis(20));
        }
        let began = Instant::now();
        let first_seq = ctl.state.lock().unwrap().seq;
        thread::sleep(std::time::Duration::from_secs(3));
        let (count, dims, slots, busy) = {
            let st = ctl.state.lock().unwrap();
            (st.seq - first_seq, st.dims, st.slots.len(), st.counters.busy_drops)
        };
        assert!(count as f64 <= began.elapsed().as_secs_f64() * 60.0 + 3.0, "too many GPU copies: {count}");
        assert_eq!(dims, expected_dims, "capture must match the test window in physical pixels (including DPI scaling)");
        assert!(slots <= MAX_GPU_SLOTS);
        assert_eq!(busy, 0, "test workload should not exhaust GPU slots");
        assert!(!root.exists(), "recording must not create any segment or clip files before saving");
        save.store(true, Ordering::Relaxed);
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        let clip = loop {
            if let Some(path) = status.lock().unwrap().last_clip.clone() { break PathBuf::from(path); }
            assert!(Instant::now() < deadline, "clip save timed out: {:?}", status.lock().unwrap().error);
            thread::sleep(std::time::Duration::from_millis(20));
        };
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER};
            SetWindowPos(window.handle(), None, 0, 0, 800, 450, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOZORDER).unwrap();
        }
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        let resized_dims = loop {
            let current_dims = ctl.state.lock().unwrap().dims;
            if current_dims != dims { break current_dims; }
            assert!(Instant::now() < deadline, "WGC did not resize with its target window");
            thread::sleep(std::time::Duration::from_millis(20));
        };
        thread::sleep(std::time::Duration::from_secs(3));
        save.store(true, Ordering::Relaxed);
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        let resized_clip = loop {
            if let Some(path) = status.lock().unwrap().last_clip.clone() {
                if Path::new(&path) != clip { break PathBuf::from(path); }
            }
            assert!(Instant::now() < deadline, "resized clip save timed out");
            thread::sleep(std::time::Duration::from_millis(20));
        };
        drop(window);
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while !ctl.is_finished() {
            assert!(Instant::now() < deadline, "closed window did not stop WGC");
            thread::sleep(std::time::Duration::from_millis(20));
        }
        ctl.stop().unwrap();
        let result = Command::new(clip::ffmpeg_path()).args(["-i"]).arg(&clip)
            .args(["-vf", "tblend=all_mode=difference,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-", "-an", "-f", "null", "-"])
            .output().unwrap();
        assert!(result.status.success(), "saved window clip must decode");
        let decoded = String::from_utf8_lossy(&result.stderr);
        assert!(decoded.contains(&format!("{}x{}", dims.0, dims.1)), "wrong output dimensions: {decoded}");
        let changed = String::from_utf8_lossy(&result.stdout).lines()
            .filter_map(|line| line.strip_prefix("lavfi.signalstats.YAVG="))
            .filter_map(|v| v.parse::<f64>().ok()).filter(|&v| v > 0.2).count();
        assert!(changed >= 80, "only {changed} changing frames in a 2-second 60 fps clip");
        let resized = Command::new(clip::ffmpeg_path()).args(["-i"]).arg(resized_clip)
            .args(["-an", "-f", "null", "-"]).output().unwrap();
        assert!(resized.status.success(), "resized clip must decode independently of the old dimensions");
        assert!(String::from_utf8_lossy(&resized.stderr).contains(&format!("{}x{}", resized_dims.0, resized_dims.1)));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn test_status() -> Arc<Mutex<RecorderStatus>> {
        Arc::new(Mutex::new(RecorderStatus {
            state: RecorderState::Recording,
            target: Some("Test".into()),
            mode: encore_common::CaptureMode::GameDetection,
            fps: 60,
            height: 0,
            buffer_secs: 20,
            hotkey: "Ctrl+Alt+K".into(),
            last_clip: None,
            error: None,
            audio_warnings: Default::default(),
        }))
    }

    /// Encode 25 seconds continuously into RAM, save once while encoding continues, then
    /// save the latest 20 seconds. Decode motion, three audio tracks and flash/beep sync.
    #[test]
    fn av_sync_and_motion_survive_continuous_ram_replay_and_concurrent_save() {
        encoder::ensure_com_mta();
        let gpu = MfGpu::new().expect("MfGpu");
        let (w, h) = (640u32, 360u32);
        let fps = 60u32;

        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        // Create a pool of textures large enough that async IMFSinkWriter never sees a reused slot
        // before it finishes reading it, even at >1000 fps unpaced test speed.
        let mut textures = Vec::new();
        let mut rtvs = Vec::new();
        for _ in 0..32 {
            let mut tex = None;
            unsafe { gpu.device.CreateTexture2D(&desc, None, Some(&mut tex)).unwrap() };
            let tex = tex.unwrap();
            let mut rtv = None;
            unsafe { gpu.device.CreateRenderTargetView(&tex, None, Some(&mut rtv)).unwrap() };
            textures.push(tex);
            rtvs.push(rtv.unwrap());
        }

        let dir = std::env::temp_dir().join(format!("encore_av_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let total_frames = 1500u64; // 25.000 s
        let t0_100ns = 10_000_000i64;
        let mut tl = AudioTimeline::new(t0_100ns);

        // Pre-populate audio chunks at `t0 + sec*1.000s + 250ms` (30 ms burst) with real silence gaps.
        for sec in 0..25i64 {
            let mut pcm = Vec::with_capacity(1440 * 4);
            for k in 0..1440 {
                let v: i16 = if (k / 24) % 2 == 0 { 18_000 } else { -18_000 };
                pcm.extend_from_slice(&v.to_le_bytes());
                pcm.extend_from_slice(&v.to_le_bytes());
            }
            tl.push(AudioChunk { start_100ns: t0_100ns + sec * 10_000_000 + 2_500_000, pcm });
        }

        let replay = Arc::new(Mutex::new(RingBuffer::new(std::time::Duration::from_secs(20), 64 * 1024 * 1024)));
        let writer = ReplayEncoder::open(&gpu, replay.clone(), (w, h), (w, h), fps, 5_000_000).unwrap();
        let mut first_save = None;

        for g in 0..total_frames {
            // Distinct color every frame, and bright white flash for 2 frames starting at `sec + 250 ms` (frame 15).
            let phase = g % fps as u64;
            let flash = phase == 15 || phase == 16;
            let color = if flash {
                [1.0f32, 1.0, 1.0, 1.0]
            } else {
                [((g % 10) as f32) / 10.0, 0.2, 0.4, 1.0]
            };
            let slot = (g as usize) % textures.len();
            unsafe {
                gpu.context.ClearRenderTargetView(&rtvs[slot], &color);
                gpu.context.Flush();
            }
            std::thread::sleep(std::time::Duration::from_millis(2));

            let v_pts = encoder::ticks_100ns(g, fps);
            let v_dur = encoder::ticks_100ns(g + 1, fps) - v_pts;
            writer.write_video(&textures[slot], v_pts, v_dur).unwrap();

            let s0 = encoder::audio_samples_at_frame(g, fps);
            let s1 = encoder::audio_samples_at_frame(g + 1, fps);
            let pcm = tl.pop_samples((s1 - s0) as usize);
            let zero_pcm = vec![0u8; pcm.len()];
            let a_pts = encoder::ticks_100ns(s0, 48_000);
            let a_dur = encoder::ticks_100ns(s1, 48_000) - a_pts;
            writer.write_audio(0, &pcm, a_pts, a_dur).unwrap();
            writer.write_audio(1, &zero_pcm, a_pts, a_dur).unwrap();
            writer.write_audio(2, &zero_pcm, a_pts, a_dur).unwrap();
            if g == 700 {
                let snapshot = writer.snapshot(std::time::Duration::from_secs(5)).unwrap();
                let output = dir.join("concurrent.mp4");
                first_save = Some(thread::spawn(move || clip::save_replay(snapshot, &output, Some("VALORANT"))));
            }
        }
        writer.finish().unwrap();
        first_save.unwrap().join().unwrap().unwrap();
        let out = dir.join("clip_20s.mp4");
        let snapshot = writer.snapshot(std::time::Duration::from_secs(20)).unwrap();
        assert!(replay.lock().unwrap().live_bytes() <= 64 * 1024 * 1024);
        clip::save_replay(snapshot, &out, Some("VALORANT")).expect("save RAM replay");

        // Use ffmpeg to inspect visual flash times (YAVG > 180) and audio non-silence times.
        let ff = clip::ffmpeg_path();
        let v_out = Command::new(&ff)
            .args([
                "-i", out.to_str().unwrap(),
                "-vf", "signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-",
                "-an", "-f", "null", "-",
            ])
            .output()
            .expect("ffmpeg video");
        let v_txt = String::from_utf8_lossy(&v_out.stdout).to_string() + &String::from_utf8_lossy(&v_out.stderr);
        let audio_streams = String::from_utf8_lossy(&v_out.stderr)
            .lines()
            .filter(|l| l.contains("Audio: aac"))
            .count();
        assert_eq!(audio_streams, 3, "expected 3 AAC audio tracks in saved clip");

        let mut flash_times = Vec::new();
        let mut cur_t: Option<f64> = None;
        for line in v_txt.lines() {
            if let Some(pos) = line.find("pts_time:") {
                cur_t = line[pos + 9..].split_whitespace().next().and_then(|s| s.parse::<f64>().ok());
            } else if let Some(val) = line.strip_prefix("lavfi.signalstats.YAVG=") {
                if val.trim().parse::<f64>().unwrap_or(0.0) > 180.0 {
                    if let Some(t) = cur_t {
                        if flash_times.last().is_none_or(|&prev| t - prev > 0.5) {
                            flash_times.push(t);
                        }
                    }
                }
            }
        }

        let a_out = Command::new(&ff)
            .args([
                "-i", out.to_str().unwrap(),
                "-af", "silencedetect=n=-30dB:d=0.1",
                "-vn", "-f", "null", "-",
            ])
            .output()
            .expect("ffmpeg audio");
        let a_txt = String::from_utf8_lossy(&a_out.stderr);
        let mut beep_times = Vec::new();
        for line in a_txt.lines() {
            if let Some(pos) = line.find("silence_end: ") {
                if let Some(t) = line[pos + 13..].split_whitespace().next().and_then(|s| s.parse::<f64>().ok()) {
                    beep_times.push(t);
                }
            }
        }

        let unique_check = Command::new(&ff)
            .args([
                "-i", out.to_str().unwrap(),
                "-vf", "tblend=all_mode=difference,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-",
                "-an", "-f", "null", "-",
            ])
            .output()
            .expect("ffmpeg unique");
        let unique_moved = String::from_utf8_lossy(&unique_check.stdout)
            .lines()
            .filter_map(|l| l.strip_prefix("lavfi.signalstats.YAVG="))
            .filter_map(|v| v.trim().parse::<f64>().ok())
            .filter(|&v| v > 0.2)
            .count();

        let _ = std::fs::remove_dir_all(&dir);
        assert!(unique_moved >= 1150, "expected >=1150 distinct frames in 20s 60fps clip, got {unique_moved}");
        assert!(flash_times.len() >= 18, "expected >=18 flashes in 20s clip, got {:?}", flash_times);
        assert!(beep_times.len() >= 18, "expected >=18 beeps in 20s clip, got {:?}", beep_times);
        for (&vf, &ab) in flash_times.iter().zip(beep_times.iter()) {
            let diff_ms = (vf - ab).abs() * 1000.0;
            assert!(diff_ms < 25.0, "AV desync {diff_ms:.1} ms at flash={vf:.3}s beep={ab:.3}s (flashes={flash_times:?}, beeps={beep_times:?})");
        }
    }

    #[test]
    #[ignore = "explicit opt-in only: records the user's primary monitor and configured audio"]
    fn monitor_capture_records_60fps_and_saves_clip() {
        use windows_capture::monitor::Monitor;

        let mut cfg = Config::default();
        let tmp_clips = std::env::temp_dir().join(format!("encore_mon_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_clips);
        std::fs::create_dir_all(&tmp_clips).unwrap();
        cfg.clips_dir = tmp_clips.clone();
        cfg.buffer_secs = 2;
        cfg.fps = 60;

        let save = Arc::new(AtomicBool::new(false));
        let mon = Monitor::primary().expect("primary monitor");
        let ctl = start(
            CaptureTarget::Monitor(mon.as_raw_hmonitor() as usize),
            Some("Desktop".into()),
            &cfg,
            save.clone(),
            test_status(),
        )
        .expect("start monitor capture");
        thread::sleep(std::time::Duration::from_secs(3));
        save.store(true, Ordering::Relaxed);
        thread::sleep(std::time::Duration::from_secs(1));
        let _ = ctl.stop();

        let clip = std::fs::read_dir(&tmp_clips)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "mp4"))
            .expect("expected saved mp4 clip");

        let ff = clip::ffmpeg_path();
        let info = Command::new(&ff)
            .args(["-i", clip.to_str().unwrap(), "-vf", "showinfo", "-an", "-f", "null", "-"])
            .output()
            .unwrap();
        let txt = String::from_utf8_lossy(&info.stderr);
        let frames = txt.lines().filter(|l| l.contains("pts_time:")).count();
        let _ = std::fs::remove_dir_all(&tmp_clips);
        assert!(frames >= 110, "expected ~120 frames in 2s 60fps clip, got {frames}");
    }

    #[test]
    fn scales_1200p_to_720p_at_120fps() {
        encoder::ensure_com_mta();
        let gpu = MfGpu::new().expect("MfGpu");
        let in_dims = (1920u32, 1200u32);
        let out_dims = encore_common::scale_dimensions(in_dims.0, in_dims.1, 720);
        assert_eq!(out_dims, (1152, 720));
        let fps = 120u32;
        let bitrate = encore_common::target_bitrate(out_dims.0, out_dims.1, fps);

        let desc = D3D11_TEXTURE2D_DESC {
            Width: in_dims.0,
            Height: in_dims.1,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut tex = None;
        unsafe { gpu.device.CreateTexture2D(&desc, None, Some(&mut tex)).unwrap() };
        let tex = tex.unwrap();

        let out = std::env::temp_dir().join(format!("encore_scale_test_{}.mp4", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let w = SampleWriter::open(&gpu, &out, in_dims, out_dims, fps, bitrate, 3).unwrap();
        let silence = vec![0u8; 48_000 * 4 / fps as usize];
        for i in 0..240u64 {
            let v_pts = encoder::ticks_100ns(i, fps);
            let v_dur = encoder::ticks_100ns(i + 1, fps) - v_pts;
            w.write_video(&tex, v_pts, v_dur).unwrap();
            let s0 = encoder::audio_samples_at_frame(i, fps);
            let s1 = encoder::audio_samples_at_frame(i + 1, fps);
            let a_pts = encoder::ticks_100ns(s0, 48_000);
            let a_dur = encoder::ticks_100ns(s1, 48_000) - a_pts;
            for t in 0..3 {
                w.write_audio(t, &silence, a_pts, a_dur).unwrap();
            }
        }
        w.finalize().unwrap();

        let ff = clip::ffmpeg_path();
        let probe = Command::new(&ff)
            .args(["-i", out.to_str().unwrap(), "-vf", "showinfo", "-an", "-f", "null", "-"])
            .output()
            .unwrap();
        let txt = String::from_utf8_lossy(&probe.stderr);
        let _ = std::fs::remove_file(&out);
        assert!(txt.contains("1152x720"), "expected 1152x720 output, got:\n{txt}");
        assert!(txt.contains("120 fps"), "expected 120 fps stream, got:\n{txt}");
        let frames = txt.lines().filter(|l| l.contains("pts_time:")).count();
        assert_eq!(frames, 240);
    }
}
