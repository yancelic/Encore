//! Hardware-accelerated MP4 segment writer using Media Foundation `IMFSinkWriter` + DXGI D3D11.
use std::path::Path;

use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Media::MediaFoundation::{
    eAVEncH264VProfile_Main, IMF2DBuffer, IMFAttributes, IMFDXGIDeviceManager, IMFSinkWriter,
    MFAudioFormat_AAC, MFAudioFormat_PCM, MFCreateAttributes, MFCreateDXGIDeviceManager,
    MFCreateDXGISurfaceBuffer, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFCreateSinkWriterFromURL, MFMediaType_Audio, MFMediaType_Video, MFShutdown, MFStartup,
    MFVideoFormat_ARGB32, MFVideoFormat_H264, MFVideoInterlace_Progressive, MFSTARTUP_NOSOCKET,
    MF_LOW_LATENCY, MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
    MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
    MF_MT_MAJOR_TYPE, MF_MT_MAX_KEYFRAME_SPACING, MF_MT_MPEG2_PROFILE, MF_MT_PIXEL_ASPECT_RATIO,
    MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SINK_WRITER_D3D_MANAGER, MF_VERSION,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::core::{HSTRING, Interface, PCWSTR};

pub type Err = Box<dyn std::error::Error + Send + Sync>;

/// Ensure COM MTA is initialized on the calling worker thread.
pub fn ensure_com_mta() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

#[inline]
fn pack_u32(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}

/// Segment frame count aligned to 32/3 seconds (10.667 s = exact 500 AAC frames) for standard FPS values.
pub fn seg_frames_for_fps(fps: u32) -> u64 {
    match fps {
        30 | 60 | 120 | 144 | 240 => (fps as u64) * 32 / 3,
        _ => (fps as u64).max(1) * 10,
    }
}

/// Total 48 kHz audio samples elapsed at the start of frame `frame_idx`.
#[inline]
pub fn audio_samples_at_frame(frame_idx: u64, fps: u32) -> u64 {
    ((frame_idx as u128 * 48_000) / fps.max(1) as u128) as u64
}

/// Convert a count in `rate` units per second into 100-ns ticks.
#[inline]
pub fn ticks_100ns(count: u64, rate: u32) -> i64 {
    ((count as i128 * 10_000_000) / rate.max(1) as i128) as i64
}

/// Shared D3D11 + Media Foundation DXGI device manager + WinRT D3D11 device.
pub struct MfGpu {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub winrt_device: IDirect3DDevice,
    pub dev_mgr: IMFDXGIDeviceManager,
}

unsafe impl Send for MfGpu {}
unsafe impl Sync for MfGpu {}

impl MfGpu {
    pub fn new() -> Result<Self, Err> {
        ensure_com_mta();
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET)?;
            let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
            let mut dev = None;
            let mut ctx = None;
            let mut level = D3D_FEATURE_LEVEL::default();
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                Default::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut dev),
                Some(&mut level),
                Some(&mut ctx),
            )?;
            let device = dev.ok_or("D3D11CreateDevice returned no device")?;
            let context = ctx.ok_or("D3D11CreateDevice returned no context")?;
            let mt: ID3D11Multithread = device.cast()?;
            let _ = mt.SetMultithreadProtected(true);

            let dxgi_dev: IDXGIDevice = device.cast()?;
            let winrt_device: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi_dev)?.cast()?;

            let mut reset_token = 0u32;
            let mut dev_mgr = None;
            MFCreateDXGIDeviceManager(&mut reset_token, &mut dev_mgr)?;
            let dev_mgr = dev_mgr.ok_or("MFCreateDXGIDeviceManager returned None")?;
            dev_mgr.ResetDevice(&device, reset_token)?;

            Ok(Self { device, context, winrt_device, dev_mgr })
        }
    }
}

impl Drop for MfGpu {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
        }
    }
}

/// Hardware-accelerated MP4 segment writer (H.264 + AAC).
pub struct SegmentWriter {
    writer: IMFSinkWriter,
    video_stream: u32,
    audio_streams: Vec<u32>,
}

// Safe across worker threads when every participating thread initializes COM MTA via `ensure_com_mta()`.
unsafe impl Send for SegmentWriter {}

impl SegmentWriter {
    pub fn open(
        gpu: &MfGpu,
        path: &Path,
        in_size: (u32, u32),
        out_size: (u32, u32),
        fps: u32,
        bitrate: u32,
        audio_tracks: usize,
    ) -> Result<Self, Err> {
        ensure_com_mta();
        let fps = fps.max(1);
        let out_h = HSTRING::from(path.as_os_str());
        unsafe {
            let mut attrs: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attrs, 4)?;
            let attrs = attrs.unwrap();
            attrs.SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, &gpu.dev_mgr)?;
            attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            attrs.SetUINT32(&MF_LOW_LATENCY, 1)?;

            let writer: IMFSinkWriter = MFCreateSinkWriterFromURL(PCWSTR(out_h.as_ptr()), None, &attrs)?;

            let v_out = MFCreateMediaType()?;
            v_out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            v_out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            v_out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
            v_out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            v_out.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Main.0 as u32)?;
            v_out.SetUINT32(&MF_MT_MAX_KEYFRAME_SPACING, fps)?;
            v_out.SetUINT64(&MF_MT_FRAME_SIZE, pack_u32(out_size.0, out_size.1))?;
            v_out.SetUINT64(&MF_MT_FRAME_RATE, pack_u32(fps, 1))?;
            v_out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u32(1, 1))?;
            let video_stream = writer.AddStream(&v_out)?;

            let v_in = MFCreateMediaType()?;
            v_in.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            v_in.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_ARGB32)?;
            v_in.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            v_in.SetUINT64(&MF_MT_FRAME_SIZE, pack_u32(in_size.0, in_size.1))?;
            v_in.SetUINT64(&MF_MT_FRAME_RATE, pack_u32(fps, 1))?;
            v_in.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u32(1, 1))?;
            writer.SetInputMediaType(video_stream, &v_in, None)?;

            let mut audio_streams = Vec::with_capacity(audio_tracks);
            for _ in 0..audio_tracks {
                let a_out = MFCreateMediaType()?;
                a_out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                a_out.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
                a_out.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                a_out.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000)?;
                a_out.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
                a_out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24_000)?;
                let a_stream = writer.AddStream(&a_out)?;

                let a_in = MFCreateMediaType()?;
                a_in.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                a_in.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
                a_in.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                a_in.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000)?;
                a_in.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
                a_in.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 192_000)?;
                a_in.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
                writer.SetInputMediaType(a_stream, &a_in, None)?;
                audio_streams.push(a_stream);
            }

            writer.BeginWriting()?;
            Ok(Self { writer, video_stream, audio_streams })
        }
    }

    pub fn write_video(&self, tex: &ID3D11Texture2D, pts_100ns: i64, dur_100ns: i64) -> Result<(), Err> {
        unsafe {
            let buf = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, tex, 0, false)?;
            let buf2d: IMF2DBuffer = buf.cast()?;
            let len = buf2d.GetContiguousLength()?;
            buf.SetCurrentLength(len)?;
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buf)?;
            sample.SetSampleTime(pts_100ns)?;
            sample.SetSampleDuration(dur_100ns)?;
            self.writer.WriteSample(self.video_stream, &sample)?;
            Ok(())
        }
    }

    pub fn write_audio(&self, track: usize, pcm: &[u8], pts_100ns: i64, dur_100ns: i64) -> Result<(), Err> {
        if pcm.is_empty() {
            return Ok(());
        }
        unsafe {
            let buf = MFCreateMemoryBuffer(pcm.len() as u32)?;
            let mut ptr = std::ptr::null_mut();
            buf.Lock(&mut ptr, None, None)?;
            std::ptr::copy_nonoverlapping(pcm.as_ptr(), ptr, pcm.len());
            buf.Unlock()?;
            buf.SetCurrentLength(pcm.len() as u32)?;
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buf)?;
            sample.SetSampleTime(pts_100ns)?;
            sample.SetSampleDuration(dur_100ns)?;
            self.writer.WriteSample(self.audio_streams[track], &sample)?;
            Ok(())
        }
    }

    pub fn finalize(self) -> Result<(), Err> {
        ensure_com_mta();
        unsafe {
            self.writer.Finalize()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_frames_align_video_and_aac_exactly() {
        for fps in [30u32, 60, 120, 144, 240] {
            let frames = seg_frames_for_fps(fps);
            let samples = audio_samples_at_frame(frames, fps);
            assert_eq!(samples, 512_000); // exact 500 * 1024 AAC frames
            assert_eq!(ticks_100ns(frames, fps), ticks_100ns(samples, 48_000));
        }
    }
}
