//! Owned WASAPI/CoreAudio streams. Callbacks never write files or wait for producers.
//! A drain receipt tracks the native playback timestamp, never human audibility.
use crate::voice_capture_store::ManagedCapture;
use crate::voice_pcm::{Resampler, s16};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
#[cfg(feature = "voice-conversation")]
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
#[cfg(feature = "voice-conversation")]
use std::time::{Duration, Instant};

fn failed(slot: &Mutex<Option<String>>, reason: &str) {
    if let Ok(mut slot) = slot.lock() {
        slot.get_or_insert_with(|| reason.to_owned());
    }
}

/// Recorder and bounded disk pump; dropping joins the pump before deleting audio.
pub struct NativeCapture {
    stream: Option<cpal::Stream>,
    pump: Option<std::thread::JoinHandle<ManagedCapture>>,
    error: Arc<Mutex<Option<String>>>,
    cap: Arc<AtomicBool>,
}
impl NativeCapture {
    /// Open the real default microphone; no synthetic production source exists.
    pub fn start(root: &std::path::Path) -> Result<Self, String> {
        let device = cpal::default_host().default_input_device()
            .ok_or("No microphone is available. Check Settings → System → Sound and microphone permissions.")?;
        let supported = device
            .default_input_config()
            .map_err(|_| "Cannot negotiate the microphone format.")?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        if config.channels == 0
            || config.channels > 32
            || !(8_000..=192_000).contains(&config.sample_rate)
        {
            return Err("Microphone PCM format exceeds supported channel/rate bounds.".into());
        }
        let mut capture = ManagedCapture::start(root).map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(32);
        let error = Arc::new(Mutex::new(None));
        let cap = Arc::new(AtomicBool::new(false));
        macro_rules! input {
            ($ty:ty) => {
                input_stream::<$ty>(&device, &config, tx, Arc::clone(&error), Arc::clone(&cap))
            };
        }
        let stream = match format {
            cpal::SampleFormat::I8 => input!(i8),
            cpal::SampleFormat::I16 => input!(i16),
            cpal::SampleFormat::I32 => input!(i32),
            cpal::SampleFormat::I64 => input!(i64),
            cpal::SampleFormat::U8 => input!(u8),
            cpal::SampleFormat::U16 => input!(u16),
            cpal::SampleFormat::U32 => input!(u32),
            cpal::SampleFormat::U64 => input!(u64),
            cpal::SampleFormat::F32 => input!(f32),
            cpal::SampleFormat::F64 => input!(f64),
            _ => return Err(
                "Microphone format is unsupported; select a PCM device in system Sound settings."
                    .into(),
            ),
        }?;
        let pump_error = Arc::clone(&error);
        let pump_cap = Arc::clone(&cap);
        let pump = std::thread::spawn(move || {
            for bytes in rx {
                match capture.write_pcm(&bytes) {
                    Ok(true) => {}
                    Ok(false) => {
                        pump_cap.store(true, Ordering::Release);
                        break;
                    }
                    Err(_) => {
                        failed(
                            &pump_error,
                            "Cannot save microphone audio; check storage permissions and disk space.",
                        );
                        break;
                    }
                }
            }
            if capture.try_finish().is_err() {
                failed(
                    &pump_error,
                    "Cannot finalize microphone audio; saved state is unconfirmed. Retry or Discard.",
                );
            }
            capture
        });
        let owner = Self {
            stream: Some(stream),
            pump: Some(pump),
            error,
            cap,
        };
        owner
            .stream
            .as_ref()
            .unwrap()
            .play()
            .map_err(|_| "Cannot start microphone; check device permissions.")?;
        Ok(owner)
    }
    /// Read-only asynchronous recorder failure, surfaced by the controller.
    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|error| error.clone())
    }
    /// A hard storage/time cap never submits an utterance.
    pub fn hit_cap(&self) -> bool {
        self.cap.load(Ordering::Acquire)
    }
    /// Stop input first, then drain already accepted packets and return owned audio.
    pub fn finish(mut self) -> (Option<ManagedCapture>, Option<String>) {
        self.stream.take();
        let capture = self.pump.take().and_then(|pump| pump.join().ok());
        let error = self.error();
        (capture, error)
    }
}
impl Drop for NativeCapture {
    fn drop(&mut self) {
        self.stream.take();
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
    }
}
fn input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: mpsc::SyncSender<Vec<u8>>,
    error: Arc<Mutex<Option<String>>>,
    cap: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    let mut resampler = Resampler::new(config.sample_rate, 16_000).map_err(str::to_owned)?;
    let stream_error = Arc::clone(&error);
    device.build_input_stream(config, move |data: &[T], _: &cpal::InputCallbackInfo| {
        if cap.load(Ordering::Acquire) { return; }
        // Even an oversized device callback produces only bounded packets.
        let frames_per_packet = (config_rate_packet_frames(channels)).max(channels);
        for packet in data.chunks(frames_per_packet) {
            let bytes = capture_packet(packet, channels, &mut resampler);
            if !bytes.is_empty() && tx.try_send(bytes).is_err() && !cap.load(Ordering::Acquire) {
                failed(&error, "Microphone buffering failed; accepted audio is retained for Retry or Discard.");
                return;
            }
        }
    }, move |_| failed(&stream_error, "Microphone stream failed; check the device and microphone permissions."), None)
        .map_err(|_| "Cannot open microphone; check the device and microphone permissions.".to_owned())
}
fn config_rate_packet_frames(channels: usize) -> usize {
    256 * channels
}
fn capture_packet<T>(packet: &[T], channels: usize, resampler: &mut Resampler) -> Vec<u8>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut bytes = Vec::with_capacity(4096);
    for frame in packet.chunks_exact(channels) {
        let value = frame
            .iter()
            .map(|sample| sample.to_sample::<f32>())
            .sum::<f32>()
            / channels as f32;
        resampler.sample(value, |value| bytes.extend_from_slice(&s16(value)));
    }
    bytes
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    #[test]
    fn native_capture_can_be_owned_by_the_recording_worker() {
        fn requires_send<T: Send>() {}
        requires_send::<NativeCapture>();
    }
    #[test]
    fn native_input_downmixes_real_pcm_and_preserves_canonical_duration() {
        let mut resampler = Resampler::new(48_000, 16_000).unwrap();
        let samples = [16384i16, 0i16].repeat(48_000);
        let mut bytes = vec![];
        for packet in samples.chunks(config_rate_packet_frames(2)) {
            let converted = capture_packet(packet, 2, &mut resampler);
            assert!(converted.len() <= 4096);
            bytes.extend(converted);
        }
        assert_eq!(bytes.len(), 32_000);
        assert!(
            bytes
                .chunks_exact(2)
                .all(|sample| sample == 8192i16.to_le_bytes())
        );
    }
    #[test]
    fn native_unsigned_input_has_correct_zero_point() {
        let mut resampler = Resampler::new(16_000, 16_000).unwrap();
        assert_eq!(
            capture_packet(&[32768u16, 32768u16], 2, &mut resampler),
            [0, 0]
        );
    }
}

#[cfg(feature = "voice-conversation")]
mod playback {
    use super::*;
    use crate::voice_playback::{MAX_QUEUED_BYTES, PlaybackError, PlaybackReceipt};
    struct Buffer {
        samples: VecDeque<f32>,
        rate: u32,
        converter: Resampler,
        written: u64,
        closing: bool,
        drained_at: Option<Instant>,
        error: bool,
    }
    struct Stream {
        _stream: cpal::Stream,
        buffer: Arc<Mutex<Buffer>>,
    }
    /// One owned native output stream per speech lane.
    #[derive(Default)]
    pub struct NativePlaybackOwner {
        current: Mutex<Option<Stream>>,
        device: String,
    }
    fn failure(reason: &str) -> PlaybackError {
        PlaybackError::Failed {
            reason: reason.to_owned(),
        }
    }
    impl Buffer {
        fn push_pcm(&mut self, pcm: &[u8]) -> Result<PlaybackReceipt, PlaybackError> {
            if !pcm.len().is_multiple_of(2) {
                return Err(failure("PCM frame is incomplete"));
            }
            if self.error {
                return Err(failure(
                    "native output stream failed; check the output device",
                ));
            }
            if self.closing {
                return Err(failure("native output stream is closing"));
            }
            let max = (MAX_QUEUED_BYTES as u64 * u64::from(self.rate) / 32_000) as usize;
            let added = (pcm.len() as u64 * u64::from(self.rate)).div_ceil(32_000) as usize;
            if self.samples.len().saturating_add(added) > max {
                return Err(PlaybackError::QueueFull {
                    max: MAX_QUEUED_BYTES,
                });
            }
            for pair in pcm.chunks_exact(2) {
                self.converter.sample(
                    f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32768.0,
                    |value| self.samples.push_back(value),
                );
            }
            self.written += pcm.len() as u64;
            Ok(PlaybackReceipt::BytesWritten(self.written))
        }
    }
    impl NativePlaybackOwner {
        pub fn new(device: String) -> Self {
            Self {
                current: Mutex::new(None),
                device,
            }
        }
        pub fn begin_stream(&self) -> Result<(), PlaybackError> {
            let mut owner = self
                .current
                .lock()
                .map_err(|_| failure("native playback lock failed"))?;
            if owner.is_some() {
                return Ok(());
            }
            let host = cpal::default_host();
            let device = if self.device.is_empty() || self.device == "default" {
                host.default_output_device()
            } else {
                host.output_devices()
                    .map_err(|_| failure("cannot enumerate output devices"))?
                    .find(|device| {
                        device.id().is_ok_and(|id| id.to_string() == self.device)
                            || device
                                .description()
                                .is_ok_and(|description| description.name() == self.device)
                    })
            }
            .ok_or(PlaybackError::NoDevice)?;
            let supported = device
                .default_output_config()
                .map_err(|_| failure("cannot negotiate the output format"))?;
            let format = supported.sample_format();
            let config: cpal::StreamConfig = supported.into();
            if config.channels == 0
                || config.channels > 32
                || !(8_000..=192_000).contains(&config.sample_rate)
            {
                return Err(failure(
                    "output PCM format exceeds supported channel/rate bounds",
                ));
            }
            let buffer = Arc::new(Mutex::new(Buffer {
                samples: VecDeque::new(),
                rate: config.sample_rate,
                converter: Resampler::new(16_000, config.sample_rate).map_err(failure)?,
                written: 0,
                closing: false,
                drained_at: None,
                error: false,
            }));
            macro_rules! output {
                ($ty:ty) => {
                    output_stream::<$ty>(&device, &config, Arc::clone(&buffer))
                };
            }
            let stream = match format {
                cpal::SampleFormat::I8 => output!(i8),
                cpal::SampleFormat::I16 => output!(i16),
                cpal::SampleFormat::I32 => output!(i32),
                cpal::SampleFormat::I64 => output!(i64),
                cpal::SampleFormat::U8 => output!(u8),
                cpal::SampleFormat::U16 => output!(u16),
                cpal::SampleFormat::U32 => output!(u32),
                cpal::SampleFormat::U64 => output!(u64),
                cpal::SampleFormat::F32 => output!(f32),
                cpal::SampleFormat::F64 => output!(f64),
                _ => {
                    return Err(failure(
                        "output format is unsupported; select a PCM device in Sound settings",
                    ));
                }
            }?;
            stream
                .play()
                .map_err(|_| failure("cannot start output device"))?;
            *owner = Some(Stream {
                _stream: stream,
                buffer,
            });
            Ok(())
        }
        pub fn push_pcm(&self, pcm: &[u8]) -> Result<PlaybackReceipt, PlaybackError> {
            if !pcm.len().is_multiple_of(2) {
                return Err(failure("PCM frame is incomplete"));
            }
            let owner = self
                .current
                .lock()
                .map_err(|_| failure("native playback lock failed"))?;
            let stream = owner
                .as_ref()
                .ok_or_else(|| failure("native playback was stopped"))?;
            let mut buffer = stream
                .buffer
                .lock()
                .map_err(|_| failure("native audio buffer failed"))?;
            buffer.push_pcm(pcm)
        }
        pub fn end_stream(&self) -> Result<PlaybackReceipt, PlaybackError> {
            let buffer = {
                let owner = self
                    .current
                    .lock()
                    .map_err(|_| failure("native playback lock failed"))?;
                let Some(stream) = owner.as_ref() else {
                    return Ok(PlaybackReceipt::Unknown);
                };
                Arc::clone(&stream.buffer)
            };
            buffer
                .lock()
                .map_err(|_| failure("native audio buffer failed"))?
                .closing = true;
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                {
                    let owner = self
                        .current
                        .lock()
                        .map_err(|_| failure("native playback lock failed"))?;
                    if !owner
                        .as_ref()
                        .is_some_and(|stream| Arc::ptr_eq(&stream.buffer, &buffer))
                    {
                        return Ok(PlaybackReceipt::Unknown);
                    }
                }
                let (failed, drained) = {
                    let buffer = buffer
                        .lock()
                        .map_err(|_| failure("native audio buffer failed"))?;
                    (
                        buffer.error,
                        buffer.drained_at.is_some_and(|time| Instant::now() >= time),
                    )
                };
                if failed {
                    self.retire(&buffer);
                    return Err(failure(
                        "native output stream failed; check the output device",
                    ));
                }
                if drained {
                    return if self.retire(&buffer) {
                        Ok(PlaybackReceipt::Drained)
                    } else {
                        Ok(PlaybackReceipt::Unknown)
                    };
                }
                if Instant::now() >= deadline {
                    self.retire(&buffer);
                    return Err(failure("native playback drain timed out"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        fn retire(&self, buffer: &Arc<Mutex<Buffer>>) -> bool {
            let old = self.current.lock().ok().and_then(|mut owner| {
                if owner
                    .as_ref()
                    .is_some_and(|stream| Arc::ptr_eq(&stream.buffer, buffer))
                {
                    owner.take()
                } else {
                    None
                }
            });
            old.is_some()
        }
        pub fn stop_flush(&self) {
            let old = self.current.lock().ok().and_then(|mut owner| owner.take());
            drop(old); // Device retirement happens after releasing the admission lock.
        }
    }
    fn output_stream<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        buffer: Arc<Mutex<Buffer>>,
    ) -> Result<cpal::Stream, PlaybackError>
    where
        T: SizedSample + FromSample<f32>,
    {
        let channels = usize::from(config.channels);
        let rate = config.sample_rate;
        let errors = Arc::clone(&buffer);
        device
            .build_output_stream(
                config,
                move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                    data.fill(T::EQUILIBRIUM);
                    let Ok(mut buffer) = buffer.try_lock() else {
                        return;
                    };
                    for frame in data.chunks_exact_mut(channels) {
                        let value = buffer.samples.pop_front().unwrap_or(0.0);
                        frame.fill(T::from_sample(value));
                    }
                    if buffer.closing && buffer.samples.is_empty() && buffer.drained_at.is_none() {
                        let timestamps = info.timestamp();
                        if let Some(delay) =
                            timestamps.playback.duration_since(&timestamps.callback)
                        {
                            let duration = Duration::from_secs_f64(
                                data.len() as f64 / channels as f64 / f64::from(rate),
                            );
                            buffer.drained_at = Some(Instant::now() + delay + duration);
                        } else {
                            buffer.error = true;
                        }
                    }
                },
                move |_| {
                    if let Ok(mut buffer) = errors.lock() {
                        buffer.error = true;
                    }
                },
                None,
            )
            .map_err(|_| {
                failure("cannot open native output device; check Sound settings and permissions")
            })
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        fn buffer(rate: u32) -> Buffer {
            Buffer {
                samples: VecDeque::new(),
                rate,
                converter: Resampler::new(16_000, rate).unwrap(),
                written: 0,
                closing: false,
                drained_at: None,
                error: false,
            }
        }
        #[test]
        fn native_output_preserves_signal_rate_and_bounded_queue() {
            for rate in [44_100, 48_000] {
                let mut buffer = buffer(rate);
                let pcm = 8192i16.to_le_bytes().repeat(16_000);
                assert_eq!(
                    buffer.push_pcm(&pcm).unwrap(),
                    PlaybackReceipt::BytesWritten(32_000)
                );
                assert_eq!(buffer.samples.len(), rate as usize);
                assert!(
                    buffer
                        .samples
                        .iter()
                        .all(|value| (*value - 0.25).abs() < 0.0001)
                );
                let before = buffer.samples.len();
                assert!(matches!(
                    buffer.push_pcm(&vec![0; MAX_QUEUED_BYTES]),
                    Err(PlaybackError::QueueFull { .. })
                ));
                assert_eq!(buffer.samples.len(), before);
                assert_eq!(buffer.written, 32_000);
            }
        }
        #[test]
        fn native_output_rejects_truncation_errors_and_late_writes() {
            let mut buffer = buffer(48_000);
            assert!(buffer.push_pcm(&[1]).is_err());
            buffer.error = true;
            assert!(buffer.push_pcm(&[0, 0]).is_err());
            buffer.error = false;
            buffer.closing = true;
            assert!(buffer.push_pcm(&[0, 0]).is_err());
            assert_eq!(buffer.written, 0);
        }
        #[test]
        fn native_idle_stop_is_idempotent_and_never_claims_drain() {
            fn requires_shared_worker<T: Send + Sync>() {}
            requires_shared_worker::<NativePlaybackOwner>();
            let owner = NativePlaybackOwner::default();
            owner.stop_flush();
            owner.stop_flush();
            assert_eq!(owner.end_stream().unwrap(), PlaybackReceipt::Unknown);
            assert!(owner.push_pcm(&[0, 0]).is_err());
        }
    }
}
#[cfg(feature = "voice-conversation")]
pub use playback::NativePlaybackOwner;
