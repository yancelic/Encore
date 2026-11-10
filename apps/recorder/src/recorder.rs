//! 3-buffer WGC capture + high-res CFR pacer + async Media Foundation segment rotation.
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use encore_common::ipc::{RecorderState, RecorderStatus};
use encore_common::Config;
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::{IInspectable, IUnknown, Interface};

use crate::audio::{self, AudioChunk, AudioTimeline};
use crate::clock::{self, HighResTimer};
use crate::encoder::{self, Err, MfGpu, SegmentWriter};
use crate::clip;

const AUDIO_LOOKBACK_100NS: i64 = 250_000; // 25 ms behind real time so WASAPI packets have arrived

/// What to capture without holding WinRT types across threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTarget {
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
    wgc_frames: u64,
    cb_total_ns: u64,
    cb_max_ns: u64,
}

impl SharedFrames {
    fn new(dims: (u32, u32)) -> Self {
        Self {
            dims,
            slots: Vec::new(),
            latest: None,
            enc_in_use: None,
            seq: 0,
            first_frame_qpc: None,
            wgc_frames: 0,
            cb_total_ns: 0,
            cb_max_ns: 0,
        }
    }
}

enum FinalizerMsg {
    FinishSegment { path: PathBuf, writer: SegmentWriter },
    ClearSegments,
    SaveClip { need: usize, secs: u32, out: PathBuf },
    Shutdown,
}

struct Stats {
    since: Instant,
    enc_frames: u32,
    new_frames: u32,
    prev_wgc: u64,
    prev_cb_ns: u64,
}

impl Stats {
    fn new() -> Self {
        Self { since: Instant::now(), enc_frames: 0, new_frames: 0, prev_wgc: 0, prev_cb_ns: 0 }
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
    (0..6).map(|_| create_gpu_texture(gpu, w, h)).collect()
}

/// COM refcount of `tex` (1 when only `SharedFrames` holds it; >1 while MF holds an `IMFSample`).
fn com_ref_count(tex: &ID3D11Texture2D) -> u32 {
    let unk: IUnknown = tex.cast().expect("IUnknown");
    unsafe {
        let _ = (Interface::vtable(&unk).AddRef)(unk.as_raw());
        (Interface::vtable(&unk).Release)(unk.as_raw())
    }
}

fn create_capture_item(target: CaptureTarget) -> Result<GraphicsCaptureItem, Err> {
    let interop: IGraphicsCaptureItemInterop =
        windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let item = match target {
        CaptureTarget::Monitor(h) => unsafe { interop.CreateForMonitor(HMONITOR(h as *mut _))? },
        CaptureTarget::Window(h) => unsafe { interop.CreateForWindow(HWND(h as *mut _))? },
    };
    Ok(item)
}

fn spawn_finalizer(
    keep: usize,
    game_name: Option<String>,
    status: Arc<Mutex<RecorderStatus>>,
) -> (mpsc::Sender<FinalizerMsg>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<FinalizerMsg>();
    let handle = thread::spawn(move || {
        encoder::ensure_com_mta();
        let mut finished: VecDeque<PathBuf> = VecDeque::new();
        while let Ok(msg) = rx.recv() {
            match msg {
                FinalizerMsg::FinishSegment { path, writer } => {
                    if let Err(e) = writer.finalize() {
                        eprintln!("segment finalize failed ({}): {e}", path.display());
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    finished.push_back(path);
                    while finished.len() > keep {
                        if let Some(old) = finished.pop_front() {
                            let _ = std::fs::remove_file(old);
                        }
                    }
                }
                FinalizerMsg::ClearSegments => {
                    for old in finished.drain(..) {
                        let _ = std::fs::remove_file(old);
                    }
                }
                FinalizerMsg::SaveClip { need, secs, out } => {
                    let skip = finished.len().saturating_sub(need);
                    let segs: Vec<PathBuf> = finished.iter().skip(skip).cloned().collect();
                    let game = game_name.clone();
                    let status_c = status.clone();
                    {
                        let mut st = status_c.lock().unwrap();
                        st.state = RecorderState::Saving;
                    }
                    thread::spawn(move || {
                        match clip::save_clip(&segs, secs, &out, game.as_deref()) {
                            Ok(()) => {
                                println!("Saved clip: {}", out.display());
                                let mut st = status_c.lock().unwrap();
                                st.last_clip = Some(out.to_string_lossy().into_owned());
                                st.error = None;
                                if st.state == RecorderState::Saving {
                                    st.state = if st.target.is_some() {
                                        RecorderState::Recording
                                    } else {
                                        RecorderState::Idle
                                    };
                                }
                            }
                            Err(e) => {
                                eprintln!("Saving clip failed: {e}");
                                let mut st = status_c.lock().unwrap();
                                st.error = Some(e);
                                if st.state == RecorderState::Saving {
                                    st.state = if st.target.is_some() {
                                        RecorderState::Recording
                                    } else {
                                        RecorderState::Idle
                                    };
                                }
                            }
                        }
                    });
                }
                FinalizerMsg::Shutdown => break,
            }
        }
    });
    (tx, handle)
}

fn spawn_prewarm(
    gpu: Arc<MfGpu>,
    path: PathBuf,
    in_dims: (u32, u32),
    target_height: u32,
    fps: u32,
) -> Receiver<Result<(PathBuf, SegmentWriter), Err>> {
    let (tx, rx) = mpsc::channel();
    let out_dims = encore_common::scale_dimensions(in_dims.0, in_dims.1, target_height);
    let bitrate = encore_common::target_bitrate(out_dims.0, out_dims.1, fps);
    thread::spawn(move || {
        let res = SegmentWriter::open(&gpu, &path, in_dims, out_dims, fps, bitrate, 3).map(|w| (path, w));
        let _ = tx.send(res);
    });
    rx
}

fn open_writer(
    gpu: &MfGpu,
    path: &Path,
    in_dims: (u32, u32),
    target_height: u32,
    fps: u32,
) -> Result<SegmentWriter, Err> {
    let out_dims = encore_common::scale_dimensions(in_dims.0, in_dims.1, target_height);
    let bitrate = encore_common::target_bitrate(out_dims.0, out_dims.1, fps);
    SegmentWriter::open(gpu, path, in_dims, out_dims, fps, bitrate, 3)
}

fn spawn_audio_track(
    kind: audio::AudioSourceKind,
    label: &'static str,
    want: Option<String>,
    stop: &Arc<AtomicBool>,
) -> (Option<Receiver<AudioChunk>>, Option<JoinHandle<()>>) {
    if want.as_deref().is_some_and(|w| w.eq_ignore_ascii_case("off")) {
        return (None, None);
    }
    let (tx, rx) = mpsc::channel();
    let stop_c = stop.clone();
    let h = thread::spawn(move || {
        if let Err(e) = audio::run(kind, label, tx, want, stop_c) {
            eprintln!("audio [{label}] unavailable (continuing silent): {e}");
        }
    });
    (Some(rx), Some(h))
}

#[allow(clippy::too_many_arguments)]
fn run_session(
    target: CaptureTarget,
    game_name: Option<String>,
    cfg: Config,
    seg_dir: PathBuf,
    save: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<SharedFrames>>,
    notify: Arc<Condvar>,
    status: Arc<Mutex<RecorderStatus>>,
) {
    encoder::ensure_com_mta();
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
    let seg_frames = encoder::seg_frames_for_fps(fps);
    let keep = (cfg.buffer_secs as u64 * fps as u64).div_ceil(seg_frames) as usize + 3;
    let (fin_tx, fin_handle) = spawn_finalizer(keep, game_name, status);

    // Pre-open seg_000000 while starting audio + WGC so Frame 0 encodes with zero startup stall.
    let mut seg_index = 0u64;
    let first_path = seg_dir.join(format!("seg_{:06}.mp4", seg_index));
    seg_index += 1;
    let mut prewarm: Option<Receiver<Result<(PathBuf, SegmentWriter), Err>>> =
        Some(spawn_prewarm(gpu.clone(), first_path, init_dims, cfg.height, fps));

    // Spawn up to 3 audio capture threads: Game (0), Chat (1), Mic (2).
    let audio_stop = Arc::new(AtomicBool::new(false));
    let mut audio_rxs: [Option<Receiver<AudioChunk>>; 3] = [None, None, None];
    let mut audio_threads: Vec<JoinHandle<()>> = Vec::new();

    let (rx0, h0) = spawn_audio_track(audio::AudioSourceKind::Loopback, "game", cfg.audio_device.clone(), &audio_stop);
    audio_rxs[0] = rx0;
    if let Some(h) = h0 { audio_threads.push(h); }

    if cfg.chat_device.as_deref().is_some_and(|s| !s.is_empty() && !s.eq_ignore_ascii_case("off")) {
        let (rx1, h1) = spawn_audio_track(audio::AudioSourceKind::Loopback, "chat", cfg.chat_device.clone(), &audio_stop);
        audio_rxs[1] = rx1;
        if let Some(h) = h1 { audio_threads.push(h); }
    }

    if cfg.mic_device.as_deref().is_some_and(|s| !s.is_empty() && !s.eq_ignore_ascii_case("off")) {
        let (rx2, h2) = spawn_audio_track(audio::AudioSourceKind::Capture, "mic", cfg.mic_device.clone(), &audio_stop);
        audio_rxs[2] = rx2;
        if let Some(h) = h2 { audio_threads.push(h); }
    }

    // WGC 3-buffer free-threaded pool with MinUpdateInterval = 0 (unlocked refresh rate).
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
            let _ = fin_tx.send(FinalizerMsg::Shutdown);
            let _ = fin_handle.join();
            return;
        }
    };
    let session = match pool.CreateCaptureSession(&item) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("CreateCaptureSession failed: {e}");
            let _ = pool.Close();
            audio_stop.store(true, Ordering::Relaxed);
            let _ = fin_tx.send(FinalizerMsg::Shutdown);
            let _ = fin_handle.join();
            return;
        }
    };
    let _ = session.SetIsBorderRequired(false);
    let _ = session.SetMinUpdateInterval(TimeSpan { Duration: 0 });

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
    let frame_tok = pool.FrameArrived(&TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new({
        let gpu = gpu.clone();
        let shared = shared.clone();
        let notify = notify.clone();
        let pool_ref = pool.clone();
        let stop = stop.clone();
        let pool_w = pool_w.clone();
        let pool_h = pool_h.clone();
        move |p_arg, _| {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let cb_t0 = Instant::now();
            let Some(p) = p_arg.as_ref() else { return Ok(()) };
            // Drain to newest available frame if more than one queued.
            let mut latest_frame = None;
            while let Ok(f) = p.TryGetNextFrame() {
                if let Some(old) = latest_frame.replace(f) {
                    let _ = old.Close();
                }
            }
            let Some(f) = latest_frame else { return Ok(()) };

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

            let surf = f.Surface()?;
            let acc: IDirect3DDxgiInterfaceAccess = surf.cast()?;
            let src_tex: ID3D11Texture2D = unsafe { acc.GetInterface()? };
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
                    } else {
                        drop(st);
                        let _ = f.Close();
                        return Ok(());
                    }
                }
                let free = (0..st.slots.len()).find(|&i| {
                    Some(i) != st.latest && Some(i) != st.enc_in_use && com_ref_count(&st.slots[i]) == 1
                });
                let slot = match free {
                    Some(i) => i,
                    None if st.slots.len() < 24 => {
                        if let Ok(extra) = create_gpu_texture(&gpu, even.0, even.1) {
                            st.slots.push(extra);
                            st.slots.len() - 1
                        } else {
                            (0..st.slots.len())
                                .find(|&i| Some(i) != st.latest && Some(i) != st.enc_in_use)
                                .unwrap_or(0)
                        }
                    }
                    None => (0..st.slots.len())
                        .find(|&i| Some(i) != st.latest && Some(i) != st.enc_in_use)
                        .unwrap_or(0),
                };
                let dst = st.slots[slot].clone();
                (slot, dst)
            };

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
                    st.latest = Some(slot);
                    st.seq += 1;
                    st.wgc_frames += 1;
                    if st.first_frame_qpc.is_none() {
                        st.first_frame_qpc = Some(frame_qpc);
                    }
                    st.cb_total_ns += cb_ns;
                    st.cb_max_ns = st.cb_max_ns.max(cb_ns);
                }
                notify.notify_one();
            }
            Ok(())
        }
    }));

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
        let _ = fin_tx.send(FinalizerMsg::Shutdown);
        let _ = fin_handle.join();
        return;
    }

    // Wait for Frame 0
    let (mut t0_100ns, mut active_dims) = {
        let mut st = shared.lock().unwrap();
        while st.latest.is_none() && !stop.load(Ordering::Relaxed) {
            st = notify.wait(st).unwrap();
        }
        (st.first_frame_qpc.unwrap_or_else(clock::qpc_100ns), st.dims)
    };

    if !stop.load(Ordering::Relaxed) {
        let mut timelines = [
            AudioTimeline::new(t0_100ns),
            AudioTimeline::new(t0_100ns),
            AudioTimeline::new(t0_100ns),
        ];
        let timer = HighResTimer::new();
        let mut stats = std::env::var_os("ENCORE_STATS").map(|_| Stats::new());

        let mut active: Option<(PathBuf, SegmentWriter)> = match prewarm.take().unwrap().recv() {
            Ok(Ok((p, w))) if active_dims == init_dims => Some((p, w)),
            Ok(Ok((p, _))) => {
                let _ = std::fs::remove_file(p);
                let p2 = seg_dir.join(format!("seg_{:06}.mp4", seg_index));
                seg_index += 1;
                open_writer(&gpu, &p2, active_dims, cfg.height, fps).ok().map(|w| (p2, w))
            }
            _ => {
                let p2 = seg_dir.join(format!("seg_{:06}.mp4", seg_index));
                seg_index += 1;
                open_writer(&gpu, &p2, active_dims, cfg.height, fps).ok().map(|w| (p2, w))
            }
        };

        let mut global_idx: u64 = 0;
        let mut local_idx: u64 = 0;
        let mut last_seq: u64 = 0;

        while !stop.load(Ordering::Relaxed) {
            let window_end_100ns = t0_100ns + encoder::ticks_100ns(global_idx + 1, fps);
            let wakeup_100ns = window_end_100ns + AUDIO_LOOKBACK_100NS;
            timer.sleep_until_100ns(wakeup_100ns);
            if stop.load(Ordering::Relaxed) {
                break;
            }

            // Guard against large OS sleep / pause (>500 ms late): skip forward cleanly without a burst.
            let now = clock::qpc_100ns();
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

            let (slot, tex, cur_dims, is_new, wgc_total, cb_ns) = {
                let mut st = shared.lock().unwrap();
                let s = match st.latest {
                    Some(s) => s,
                    None => continue,
                };
                st.enc_in_use = Some(s);
                let tex = st.slots[s].clone();
                let is_new = st.seq != last_seq;
                last_seq = st.seq;
                (s, tex, st.dims, is_new, st.wgc_frames, st.cb_total_ns)
            };

            // Handle resize cleanly (segments of different resolutions cannot concat).
            if cur_dims != active_dims {
                if let Some((path, writer)) = active.take() {
                    let _ = fin_tx.send(FinalizerMsg::FinishSegment { path, writer });
                }
                let _ = fin_tx.send(FinalizerMsg::ClearSegments);
                if let Some(Ok(Ok((p, _)))) = prewarm.take().map(|rx| rx.recv()) {
                    let _ = std::fs::remove_file(p);
                }
                active_dims = cur_dims;
                local_idx = 0;
                let p = seg_dir.join(format!("seg_{:06}.mp4", seg_index));
                seg_index += 1;
                match open_writer(&gpu, &p, active_dims, cfg.height, fps) {
                    Ok(w) => active = Some((p, w)),
                    Err(e) => {
                        eprintln!("reopen segment failed after resize: {e}");
                        shared.lock().unwrap().enc_in_use = None;
                        break;
                    }
                }
            }

            let prewarm_at = seg_frames.saturating_sub((fps as u64 * 3 / 2).min(seg_frames / 2));
            if local_idx >= prewarm_at && prewarm.is_none() {
                let next_path = seg_dir.join(format!("seg_{:06}.mp4", seg_index));
                seg_index += 1;
                prewarm = Some(spawn_prewarm(gpu.clone(), next_path, active_dims, cfg.height, fps));
            }

            let s0 = encoder::audio_samples_at_frame(local_idx, fps);
            let s1 = encoder::audio_samples_at_frame(local_idx + 1, fps);
            let a_pts = encoder::ticks_100ns(s0, 48_000);
            let a_dur = encoder::ticks_100ns(s1, 48_000) - a_pts;
            let sample_count = (s1 - s0) as usize;
            let pcms: [Vec<u8>; 3] = std::array::from_fn(|t| timelines[t].pop_samples(sample_count));

            if let Some((_, ref writer)) = active {
                let v_pts = encoder::ticks_100ns(local_idx, fps);
                let v_dur = encoder::ticks_100ns(local_idx + 1, fps) - v_pts;
                if let Err(e) = writer.write_video(&tex, v_pts, v_dur) {
                    eprintln!("write_video failed: {e}");
                }
                for (t, pcm) in pcms.iter().enumerate() {
                    if let Err(e) = writer.write_audio(t, pcm, a_pts, a_dur) {
                        eprintln!("write_audio[{t}] failed: {e}");
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

            if let Some(st_out) = stats.as_mut() {
                st_out.enc_frames += 1;
                if is_new {
                    st_out.new_frames += 1;
                }
                let el = st_out.since.elapsed().as_secs_f64();
                if el >= 5.0 {
                    let dwgc = wgc_total - st_out.prev_wgc;
                    let dcb = cb_ns - st_out.prev_cb_ns;
                    let out_dims = encore_common::scale_dimensions(active_dims.0, active_dims.1, cfg.height);
                    println!(
                        "[stats] WGC {:.1} fps in | enc {:.1} fps ({:.0}% new) | WGC cb avg {:.2} ms | {}x{} -> {}x{}",
                        dwgc as f64 / el,
                        st_out.enc_frames as f64 / el,
                        (st_out.new_frames as f64 * 100.0) / st_out.enc_frames.max(1) as f64,
                        (dcb as f64 / 1_000_000.0) / dwgc.max(1) as f64,
                        active_dims.0,
                        active_dims.1,
                        out_dims.0,
                        out_dims.1
                    );
                    st_out.prev_wgc = wgc_total;
                    st_out.prev_cb_ns = cb_ns;
                    st_out.enc_frames = 0;
                    st_out.new_frames = 0;
                    st_out.since = Instant::now();
                }
            }

            let want_save = save.swap(false, Ordering::Relaxed);
            if want_save || local_idx >= seg_frames {
                if let Some((path, writer)) = active.take() {
                    let _ = fin_tx.send(FinalizerMsg::FinishSegment { path, writer });
                }
                if want_save {
                    let need = (cfg.buffer_secs as u64 * fps as u64).div_ceil(seg_frames) as usize + 1;
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let out = cfg.clips_dir.join(format!("Encore_{stamp}.mp4"));
                    let _ = fin_tx.send(FinalizerMsg::SaveClip { need, secs: cfg.buffer_secs, out });
                }
                local_idx = 0;
                active = match prewarm.take().and_then(|rx| rx.recv().ok()) {
                    Some(Ok(pair)) => Some(pair),
                    _ => {
                        let p = seg_dir.join(format!("seg_{:06}.mp4", seg_index));
                        seg_index += 1;
                        open_writer(&gpu, &p, active_dims, cfg.height, fps).ok().map(|w| (p, w))
                    }
                };
            }
        }

        if let Some((path, writer)) = active.take() {
            let _ = fin_tx.send(FinalizerMsg::FinishSegment { path, writer });
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
    let _ = fin_tx.send(FinalizerMsg::Shutdown);
    let _ = fin_handle.join();
}

/// Start capturing `target` (a monitor or a window) in the background.
pub fn start(
    target: CaptureTarget,
    game_name: Option<String>,
    cfg: &Config,
    save: Arc<AtomicBool>,
    status: Arc<Mutex<RecorderStatus>>,
) -> Result<CaptureControl, Err> {
    let segments = Config::dir().join("segments");
    if let Ok(rd) = std::fs::read_dir(&segments) {
        for e in rd.flatten() {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seg_dir = segments.join(stamp.to_string());
    std::fs::create_dir_all(&seg_dir)?;

    let stop = Arc::new(AtomicBool::new(false));
    let notify = Arc::new(Condvar::new());
    let shared = Arc::new(Mutex::new(SharedFrames::new((0, 0))));

    let cfg_c = cfg.clone();
    let stop_c = stop.clone();
    let shared_c = shared.clone();
    let notify_c = notify.clone();
    let thread = thread::spawn(move || {
        run_session(target, game_name, cfg_c, seg_dir, save, stop_c, shared_c, notify_c, status);
    });

    Ok(CaptureControl { stop, notify, state: shared, thread: Some(thread) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

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
        }))
    }

    /// End-to-end 25s clapper test: flashes GPU texture bright white + emits a 1 kHz audio beep
    /// every 1.000 s (with silence gaps in between), rolls across 2 segment boundaries (10.67s & 21.33s),
    /// joins + trims to the last 20s with `clip::save_clip`, and verifies via ffmpeg that all 3 audio
    /// tracks survive and every audio beep aligns with its visual flash within 25 ms.
    #[test]
    fn av_sync_and_fps_across_segment_rotations() {
        encoder::ensure_com_mta();
        let gpu = MfGpu::new().expect("MfGpu");
        let (w, h) = (640u32, 360u32);
        let fps = 60u32;
        let seg_frames = encoder::seg_frames_for_fps(fps); // 640 frames = 10.6667s

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

        let mut segs = Vec::new();
        let mut seg_idx = 0u64;
        let mut local_idx = 0u64;
        let mut seg_path = dir.join(format!("seg_{:06}.mp4", seg_idx));
        let mut writer = Some(SegmentWriter::open(&gpu, &seg_path, (w, h), (w, h), fps, 5_000_000, 3).unwrap());

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

            let v_pts = encoder::ticks_100ns(local_idx, fps);
            let v_dur = encoder::ticks_100ns(local_idx + 1, fps) - v_pts;
            writer.as_ref().unwrap().write_video(&textures[slot], v_pts, v_dur).unwrap();

            let s0 = encoder::audio_samples_at_frame(local_idx, fps);
            let s1 = encoder::audio_samples_at_frame(local_idx + 1, fps);
            let pcm = tl.pop_samples((s1 - s0) as usize);
            let zero_pcm = vec![0u8; pcm.len()];
            let a_pts = encoder::ticks_100ns(s0, 48_000);
            let a_dur = encoder::ticks_100ns(s1, 48_000) - a_pts;
            writer.as_ref().unwrap().write_audio(0, &pcm, a_pts, a_dur).unwrap();
            writer.as_ref().unwrap().write_audio(1, &zero_pcm, a_pts, a_dur).unwrap();
            writer.as_ref().unwrap().write_audio(2, &zero_pcm, a_pts, a_dur).unwrap();

            local_idx += 1;
            if local_idx >= seg_frames {
                writer.take().unwrap().finalize().unwrap();
                segs.push(seg_path);
                seg_idx += 1;
                local_idx = 0;
                seg_path = dir.join(format!("seg_{:06}.mp4", seg_idx));
                writer = Some(SegmentWriter::open(&gpu, &seg_path, (w, h), (w, h), fps, 5_000_000, 3).unwrap());
            }
        }
        if let Some(w_last) = writer.take() {
            w_last.finalize().unwrap();
            segs.push(seg_path);
        }

        let out = dir.join("clip_20s.mp4");
        clip::save_clip(&segs, 20, &out, Some("VALORANT")).expect("save_clip");

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
        let w = SegmentWriter::open(&gpu, &out, in_dims, out_dims, fps, bitrate, 3).unwrap();
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
