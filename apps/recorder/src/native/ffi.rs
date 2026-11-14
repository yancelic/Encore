//! The native library owns OBS; encoded bytes immediately enter Encore's shared RAM budget.
use encore_encode::{PacketInfo, RingBuffer, Snapshot};
use libloading::Library;
use serde_json::Value;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[repr(C)]
pub struct PacketView {
    pub stream: u32,
    pub pts: i64,
    pub dts: i64,
    pub duration: i64,
    pub data: *const u8,
    pub size: usize,
    pub keyframe: u8,
}

#[repr(C)]
#[derive(Default)]
pub struct FormatView {
    pub width: u32,
    pub height: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub extra_data: *const u8,
    pub extra_data_size: usize,
}

pub struct Format {
    width: u32,
    height: u32,
    sample_rate: u32,
    channels: u32,
    extra_data: Vec<u8>,
}

impl Format {
    fn view(&self) -> FormatView {
        FormatView {
            width: self.width,
            height: self.height,
            sample_rate: self.sample_rate,
            channels: self.channels,
            extra_data: self.extra_data.as_ptr(),
            extra_data_size: self.extra_data.len(),
        }
    }
}

type PacketCallback = unsafe extern "C" fn(*mut c_void, *const PacketView);
type Create = unsafe extern "C" fn(*const c_char, PacketCallback, *mut c_void) -> *mut c_void;
type Request = unsafe extern "C" fn(*mut c_void, *const c_char) -> *const c_char;
type Tick = unsafe extern "C" fn(*mut c_void);
type GetFormat = unsafe extern "C" fn(*mut c_void, u32, *mut FormatView) -> i32;
type Destroy = unsafe extern "C" fn(*mut c_void);
type LastError = unsafe extern "C" fn() -> *const c_char;
type Mux = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    *const FormatView,
    *const PacketView,
    usize,
    *mut c_char,
    usize,
) -> i32;

pub struct Api {
    // Keep the library mapped until every accepted save finishes.
    _library: Library,
    create: Create,
    request: Request,
    tick: Tick,
    get_format: GetFormat,
    destroy: Destroy,
    last_error: LastError,
    mux: Mux,
}

impl Api {
    pub fn load(path: &Path) -> Result<Arc<Self>, String> {
        unsafe {
            let library = Library::new(path)
                .map_err(|e| format!("Could not load native capture runtime: {e}"))?;
            macro_rules! symbol {
                ($name:literal) => {
                    *library
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|e| format!("Incomplete capture runtime: {e}"))?
                };
            }
            let api = Self {
                create: symbol!("ec_create"),
                request: symbol!("ec_request"),
                tick: symbol!("ec_tick"),
                get_format: symbol!("ec_get_format"),
                destroy: symbol!("ec_destroy"),
                last_error: symbol!("ec_last_error"),
                mux: symbol!("ec_mux"),
                _library: library,
            };
            Ok(Arc::new(api))
        }
    }

    pub fn mux(
        &self,
        path: &Path,
        game: &str,
        formats: &[Format],
        snapshot: &Snapshot,
    ) -> Result<(), String> {
        if formats.len() != encore_encode::STREAMS {
            return Err("Incomplete replay formats".into());
        }
        let path = CString::new(path.to_str().ok_or("Clip path must be valid UTF-8")?)
            .map_err(|e| e.to_string())?;
        let game = CString::new(game).map_err(|e| e.to_string())?;
        let formats: Vec<_> = formats.iter().map(Format::view).collect();
        let packets: Vec<_> = snapshot
            .streams
            .iter()
            .flatten()
            .map(|packet| PacketView {
                stream: packet.info.stream as u32,
                pts: packet.info.pts,
                dts: packet.info.dts,
                duration: packet.info.duration,
                data: packet.data.bytes().as_ptr(),
                size: packet.data.bytes().len(),
                keyframe: u8::from(packet.info.keyframe),
            })
            .collect();
        let mut error = [0u8; 2048];
        let ok = unsafe {
            (self.mux)(
                path.as_ptr(),
                game.as_ptr(),
                formats.as_ptr(),
                packets.as_ptr(),
                packets.len(),
                error.as_mut_ptr().cast(),
                error.len(),
            )
        };
        if ok != 0 {
            return Ok(());
        }
        Err(unsafe { CStr::from_ptr(error.as_ptr().cast()) }
            .to_string_lossy()
            .into_owned())
    }
}

unsafe extern "C" fn receive_packet(context: *mut c_void, packet: *const PacketView) {
    // Never unwind across the C ABI. Engine destruction stops and drains OBS callbacks
    // before the callback context can be dropped.
    let _ = std::panic::catch_unwind(|| {
        if context.is_null() || packet.is_null() {
            return;
        }
        let replay = &*(context.cast::<Arc<Mutex<RingBuffer>>>());
        let packet = &*packet;
        let Ok(mut replay) = replay.lock() else {
            return;
        };
        if packet.stream == u32::MAX {
            replay.clear();
            return;
        }
        if packet.stream as usize >= encore_encode::STREAMS
            || packet.data.is_null()
            || packet.size == 0
            || packet.size > 32 * 1024 * 1024
        {
            return;
        }
        let bytes = std::slice::from_raw_parts(packet.data, packet.size).to_vec();
        replay.push(
            PacketInfo {
                stream: packet.stream as usize,
                pts: packet.pts,
                dts: packet.dts,
                duration: packet.duration,
                keyframe: packet.keyframe != 0,
            },
            bytes,
        );
    });
}

pub struct Engine {
    api: Arc<Api>,
    handle: *mut c_void,
    _callback: Box<Arc<Mutex<RingBuffer>>>,
}

impl Engine {
    pub fn open(
        api: Arc<Api>,
        runtime: &Path,
        replay: Arc<Mutex<RingBuffer>>,
    ) -> Result<Self, String> {
        let path = CString::new(runtime.to_str().ok_or("Runtime path must be valid UTF-8")?)
            .map_err(|e| e.to_string())?;
        let mut callback = Box::new(replay);
        let handle = unsafe {
            (api.create)(
                path.as_ptr(),
                receive_packet,
                (&mut *callback as *mut Arc<Mutex<RingBuffer>>).cast(),
            )
        };
        if handle.is_null() {
            let error = unsafe { (api.last_error)() };
            return Err(if error.is_null() {
                "Native capture initialization failed".into()
            } else {
                unsafe { CStr::from_ptr(error) }
                    .to_string_lossy()
                    .into_owned()
            });
        }
        Ok(Self {
            api,
            handle,
            _callback: callback,
        })
    }

    pub fn request(&mut self, request: &Value) -> Result<Value, String> {
        let request = CString::new(request.to_string()).map_err(|e| e.to_string())?;
        let answer = unsafe { (self.api.request)(self.handle, request.as_ptr()) };
        if answer.is_null() {
            return Err("Missing native engine response".into());
        }
        serde_json::from_slice(unsafe { CStr::from_ptr(answer) }.to_bytes())
            .map_err(|e| e.to_string())
    }

    pub fn tick(&mut self) {
        unsafe { (self.api.tick)(self.handle) };
    }

    pub fn formats(&self) -> Result<Vec<Format>, String> {
        (0..encore_encode::STREAMS)
            .map(|stream| {
                let mut format = FormatView::default();
                if unsafe { (self.api.get_format)(self.handle, stream as u32, &mut format) } == 0
                    || format.extra_data.is_null()
                    || format.extra_data_size == 0
                    || format.extra_data_size > 1024 * 1024
                {
                    return Err("Replay is warming up; wait for the first encoded frames".into());
                }
                Ok(Format {
                    width: format.width,
                    height: format.height,
                    sample_rate: format.sample_rate,
                    channels: format.channels,
                    extra_data: unsafe {
                        std::slice::from_raw_parts(format.extra_data, format.extra_data_size)
                    }
                    .to_vec(),
                })
            })
            .collect()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe { (self.api.destroy)(self.handle) };
    }
}
