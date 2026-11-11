//! A rateless Media Foundation sink receiving compressed H.264/AAC samples in RAM.
//! Sink Writer still owns hardware encoding/conversion; this sink performs no disk I/O.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use encore_encode::{PacketInfo, RingBuffer, STREAMS};
use windows::core::{implement, IUnknown, IUnknownImpl, Ref, Result, GUID, HRESULT};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;

struct StreamState {
    id: u32,
    stream: IMFStreamSink,
    handler: IMFMediaTypeHandler,
    events: IMFMediaEventQueue,
}

struct SinkState {
    parent: Mutex<Option<IMFMediaSink>>,
    streams: Mutex<Vec<StreamState>>,
    clock: Mutex<Option<IMFPresentationClock>>,
    stopped: AtomicBool,
    replay: Arc<Mutex<RingBuffer>>,
}

// MF event queues, type handlers and the presentation clock are used through their
// thread-safe COM APIs; mutable collections are locked. All callers initialize COM MTA.
unsafe impl Send for SinkState {}
unsafe impl Sync for SinkState {}

impl SinkState {
    fn check(&self) -> Result<()> {
        if self.stopped.load(Ordering::Acquire) {
            Err(MF_E_SHUTDOWN.into())
        } else {
            Ok(())
        }
    }

    fn event(&self, kind: MF_EVENT_TYPE, request: bool) -> Result<()> {
        self.check()?;
        let queues: Vec<_> = self.streams.lock().unwrap().iter().map(|s| s.events.clone()).collect();
        for queue in queues {
            queue_event(&queue, kind, std::ptr::null())?;
            if request {
                queue_event(&queue, MEStreamSinkRequestSample, std::ptr::null())?;
            }
        }
        Ok(())
    }
}

fn queue_event(queue: &IMFMediaEventQueue, kind: MF_EVENT_TYPE, value: *const PROPVARIANT) -> Result<()> {
    unsafe { queue.QueueEventParamVar(kind.0 as u32, &GUID::zeroed(), HRESULT(0), value) }
}

/// Own the sink independently of Sink Writer, whose destruction does not call Shutdown.
pub struct PacketSink {
    pub interface: IMFMediaSink,
    state: Arc<SinkState>,
}

impl PacketSink {
    pub fn new(replay: Arc<Mutex<RingBuffer>>) -> Self {
        let state = Arc::new(SinkState {
            parent: Mutex::new(None),
            streams: Mutex::new(Vec::new()),
            clock: Mutex::new(None),
            stopped: AtomicBool::new(false),
            replay,
        });
        let interface: IMFMediaSink = MemorySink { state: state.clone() }.into();
        *state.parent.lock().unwrap() = Some(interface.clone());
        Self { interface, state }
    }

    pub fn formats(&self) -> Result<Vec<IMFMediaType>> {
        self.state.check()?;
        self.state
            .streams
            .lock()
            .unwrap()
            .iter()
            .map(|stream| unsafe {
                let current = stream.handler.GetCurrentMediaType()?;
                let copy = MFCreateMediaType()?;
                current.CopyAllItems(&copy)?;
                Ok(copy)
            })
            .collect()
    }
}

impl Drop for PacketSink {
    fn drop(&mut self) {
        unsafe {
            let _ = self.interface.Shutdown();
        }
    }
}

#[implement(IMFMediaSink, IMFClockStateSink)]
struct MemorySink {
    state: Arc<SinkState>,
}

impl IMFMediaSink_Impl for MemorySink_Impl {
    fn GetCharacteristics(&self) -> Result<u32> {
        self.state.check()?;
        Ok(MEDIASINK_RATELESS)
    }

    fn AddStreamSink(&self, id: u32, media_type: Ref<IMFMediaType>) -> Result<IMFStreamSink> {
        self.state.check()?;
        let mut streams = self.state.streams.lock().unwrap();
        if streams.iter().any(|s| s.id == id) {
            return Err(MF_E_STREAMSINK_EXISTS.into());
        }
        if streams.len() >= STREAMS {
            return Err(MF_E_INVALIDSTREAMNUMBER.into());
        }
        let handler = unsafe { MFCreateSimpleTypeHandler()? };
        if let Some(media_type) = media_type.as_ref() {
            unsafe {
                handler.SetCurrentMediaType(media_type)?;
            }
        }
        let events = unsafe { MFCreateEventQueue()? };
        let stream: IMFStreamSink = MemoryStream {
            id,
            index: streams.len(),
            parent: Arc::downgrade(&self.state),
            handler: handler.clone(),
            events: events.clone(),
        }
        .into();
        streams.push(StreamState { id, stream: stream.clone(), handler, events });
        Ok(stream)
    }

    fn RemoveStreamSink(&self, id: u32) -> Result<()> {
        self.state.check()?;
        let mut streams = self.state.streams.lock().unwrap();
        let index = streams
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| windows::core::Error::from_hresult(MF_E_INVALIDSTREAMNUMBER))?;
        let stream = streams.remove(index);
        unsafe { stream.events.Shutdown() }
    }

    fn GetStreamSinkCount(&self) -> Result<u32> {
        self.state.check()?;
        Ok(self.state.streams.lock().unwrap().len() as u32)
    }
    fn GetStreamSinkByIndex(&self, index: u32) -> Result<IMFStreamSink> {
        self.state.check()?;
        self.state
            .streams
            .lock()
            .unwrap()
            .get(index as usize)
            .map(|s| s.stream.clone())
            .ok_or_else(|| MF_E_INVALIDSTREAMNUMBER.into())
    }
    fn GetStreamSinkById(&self, id: u32) -> Result<IMFStreamSink> {
        self.state.check()?;
        self.state
            .streams
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.stream.clone())
            .ok_or_else(|| MF_E_INVALIDSTREAMNUMBER.into())
    }
    fn SetPresentationClock(&self, clock: Ref<IMFPresentationClock>) -> Result<()> {
        self.state.check()?;
        let callback: IMFClockStateSink = self.to_interface();
        let previous = self.state.clock.lock().unwrap().take();
        if let Some(previous) = previous {
            unsafe {
                previous.RemoveClockStateSink(&callback)?;
            }
        }
        if let Some(clock) = clock.as_ref() {
            unsafe {
                clock.AddClockStateSink(&callback)?;
            }
            *self.state.clock.lock().unwrap() = Some(clock.clone());
        }
        Ok(())
    }
    fn GetPresentationClock(&self) -> Result<IMFPresentationClock> {
        self.state.check()?;
        self.state.clock.lock().unwrap().clone().ok_or_else(|| MF_E_NO_CLOCK.into())
    }
    fn Shutdown(&self) -> Result<()> {
        if self.state.stopped.swap(true, Ordering::AcqRel) {
            return Err(MF_E_SHUTDOWN.into());
        }
        if let Some(clock) = self.state.clock.lock().unwrap().take() {
            let callback: IMFClockStateSink = self.to_interface();
            unsafe {
                let _ = clock.RemoveClockStateSink(&callback);
            }
        }
        let streams = std::mem::take(&mut *self.state.streams.lock().unwrap());
        for stream in streams {
            unsafe {
                let _ = stream.events.Shutdown();
            }
        }
        self.state.parent.lock().unwrap().take();
        Ok(())
    }
}

impl IMFClockStateSink_Impl for MemorySink_Impl {
    fn OnClockStart(&self, _: i64, _: i64) -> Result<()> {
        self.state.event(MEStreamSinkStarted, true)
    }
    fn OnClockStop(&self, _: i64) -> Result<()> {
        self.state.event(MEStreamSinkStopped, false)
    }
    fn OnClockPause(&self, _: i64) -> Result<()> {
        self.state.event(MEStreamSinkPaused, false)
    }
    fn OnClockRestart(&self, _: i64) -> Result<()> {
        self.state.event(MEStreamSinkStarted, true)
    }
    fn OnClockSetRate(&self, _: i64, _: f32) -> Result<()> {
        self.state.check()
    }
}

#[implement(IMFStreamSink)]
struct MemoryStream {
    id: u32,
    index: usize,
    parent: Weak<SinkState>,
    handler: IMFMediaTypeHandler,
    events: IMFMediaEventQueue,
}

// See SinkState's thread-safety contract; the stream only owns thread-safe MF helpers.
unsafe impl Send for MemoryStream {}
unsafe impl Sync for MemoryStream {}

impl MemoryStream {
    fn parent(&self) -> Result<Arc<SinkState>> {
        let parent = self.parent.upgrade().ok_or_else(|| windows::core::Error::from_hresult(MF_E_SHUTDOWN))?;
        parent.check()?;
        Ok(parent)
    }
}

impl IMFMediaEventGenerator_Impl for MemoryStream_Impl {
    fn GetEvent(&self, flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> Result<IMFMediaEvent> {
        unsafe { self.events.GetEvent(flags.0) }
    }
    fn BeginGetEvent(&self, callback: Ref<IMFAsyncCallback>, state: Ref<IUnknown>) -> Result<()> {
        unsafe { self.events.BeginGetEvent(callback.ok()?, state.as_ref()) }
    }
    fn EndGetEvent(&self, result: Ref<IMFAsyncResult>) -> Result<IMFMediaEvent> {
        unsafe { self.events.EndGetEvent(result.ok()?) }
    }
    fn QueueEvent(&self, kind: u32, extended: *const GUID, status: HRESULT, value: *const PROPVARIANT) -> Result<()> {
        unsafe { self.events.QueueEventParamVar(kind, extended, status, value) }
    }
}

impl IMFStreamSink_Impl for MemoryStream_Impl {
    fn GetMediaSink(&self) -> Result<IMFMediaSink> {
        self.parent()?.parent.lock().unwrap().clone().ok_or_else(|| MF_E_SHUTDOWN.into())
    }
    fn GetIdentifier(&self) -> Result<u32> {
        self.parent()?;
        Ok(self.id)
    }
    fn GetMediaTypeHandler(&self) -> Result<IMFMediaTypeHandler> {
        self.parent()?;
        Ok(self.handler.clone())
    }
    fn ProcessSample(&self, sample: Ref<IMFSample>) -> Result<()> {
        let parent = self.parent()?;
        let sample = sample.ok()?;
        let info = unsafe {
            let pts = sample.GetSampleTime()?;
            let duration = match sample.GetSampleDuration() {
                Ok(duration) if duration > 0 => duration,
                _ => {
                    let format = self.handler.GetCurrentMediaType()?;
                    if self.index == 0 {
                        let rate = format.GetUINT64(&MF_MT_FRAME_RATE)?;
                        10_000_000 * (rate as u32).max(1) as i64 / ((rate >> 32) as i64).max(1)
                    } else {
                        10_000_000 * 1024 / format.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)?.max(1) as i64
                    }
                }
            };
            PacketInfo {
                stream: self.index,
                pts,
                dts: sample.GetUINT64(&MFSampleExtension_DecodeTimestamp).map(|t| t as i64).unwrap_or(pts),
                duration,
                keyframe: sample.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0,
            }
        };
        let data = unsafe {
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut address = std::ptr::null_mut();
            let mut length = 0;
            buffer.Lock(&mut address, None, Some(&mut length))?;
            let data =
                if length == 0 { Vec::new() } else { std::slice::from_raw_parts(address, length as usize).to_vec() };
            buffer.Unlock()?;
            data
        };
        parent.replay.lock().unwrap().push(info, data);
        queue_event(&self.events, MEStreamSinkRequestSample, std::ptr::null())
    }
    fn PlaceMarker(
        &self,
        _: MFSTREAMSINK_MARKER_TYPE,
        _: *const PROPVARIANT,
        context: *const PROPVARIANT,
    ) -> Result<()> {
        self.parent()?;
        queue_event(&self.events, MEStreamSinkMarker, context)
    }
    fn Flush(&self) -> Result<()> {
        self.parent()?;
        Ok(())
    }
}
