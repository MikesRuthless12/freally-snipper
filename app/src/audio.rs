//! System/loopback + microphone audio capture for screen recording (P5.2), via
//! cpal.
//!
//! - **System audio** ("what you hear"): **Windows** = WASAPI **loopback** of the
//!   default output device; **Linux** = a PulseAudio/PipeWire `*.monitor` input
//!   source; **macOS** = no native loopback (needs a virtual device such as
//!   BlackHole, which the user selects as the microphone) — so it is unavailable
//!   here and the UI says so.
//! - **Microphone**: the default input device.
//!
//! Each enabled source is resampled to a common **48 kHz stereo i16** stream and,
//! when both are on, **mixed**, then fed to the recorder's `.fvid` audio track.
//! Audio is **best-effort**: if a source can't open, recording simply proceeds
//! without it (and with neither, the recording is video-only).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Host, SampleFormat, Stream, StreamConfig};

/// Owned output format of the mixed audio track.
const TARGET_RATE: u32 = 48_000;
const TARGET_CHANNELS: u16 = 2;

/// Cap each source buffer at ~2 s so a paused/stalled recorder can't grow it
/// without bound (old audio is dropped rather than leaked).
const MAX_BUFFERED: usize = (TARGET_RATE as usize) * (TARGET_CHANNELS as usize) * 2;

/// Which audio sources to record.
#[derive(Clone, Copy, Debug)]
pub struct AudioConfig {
    /// Capture system/loopback audio ("what you hear").
    pub system: bool,
    /// Capture the microphone.
    pub mic: bool,
}

impl AudioConfig {
    /// Whether any source is requested.
    pub fn any(self) -> bool {
        self.system || self.mic
    }
}

/// A source's rolling buffer of resampled 48 kHz stereo i16 samples, shared with
/// its cpal callback thread.
type SourceBuf = Arc<Mutex<VecDeque<i16>>>;

/// Live audio capture: holds the cpal streams (kept alive) and the mixer buffers.
/// Created and dropped on the recorder worker thread (cpal streams are `!Send`).
pub struct AudioCapture {
    sources: Vec<SourceBuf>,
    _streams: Vec<Stream>,
}

impl AudioCapture {
    /// Start capture for the requested sources. Returns `None` if none could be
    /// opened, so the caller falls back to a video-only recording.
    pub fn start(config: AudioConfig) -> Option<Self> {
        let host = cpal::default_host();
        let mut sources = Vec::new();
        let mut streams = Vec::new();

        if config.system {
            if let Some((device, use_output_config)) = system_device(&host) {
                if let Some((buf, stream)) = open_source(&device, use_output_config) {
                    sources.push(buf);
                    streams.push(stream);
                }
            }
        }
        if config.mic {
            if let Some(device) = host.default_input_device() {
                if let Some((buf, stream)) = open_source(&device, false) {
                    sources.push(buf);
                    streams.push(stream);
                }
            }
        }

        if sources.is_empty() {
            return None;
        }
        for stream in &streams {
            if let Err(err) = stream.play() {
                eprintln!("Freally Snipper: couldn't start an audio stream: {err}");
            }
        }
        Some(Self {
            sources,
            _streams: streams,
        })
    }

    /// Sample rate of the produced track.
    pub fn sample_rate(&self) -> u32 {
        TARGET_RATE
    }

    /// Channel count of the produced track.
    pub fn channels(&self) -> u16 {
        TARGET_CHANNELS
    }

    /// Drain and mix the samples currently available across all sources, aligned
    /// to the source with the fewest buffered samples so they stay in sync.
    pub fn drain(&self) -> Vec<i16> {
        let mut guards: Vec<_> = self.sources.iter().filter_map(|s| s.lock().ok()).collect();
        if guards.is_empty() {
            return Vec::new();
        }
        let common = guards.iter().map(|g| g.len()).min().unwrap_or(0);
        // Keep whole stereo frames.
        let common = common - (common % TARGET_CHANNELS as usize);
        if common == 0 {
            return Vec::new();
        }
        // Mix in i32 to avoid clipping on the sum, then clamp back to i16.
        let mut mixed = vec![0i32; common];
        for guard in guards.iter_mut() {
            for (acc, s) in mixed.iter_mut().zip(guard.drain(..common)) {
                *acc += s as i32;
            }
        }
        mixed
            .into_iter()
            .map(|v| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16)
            .collect()
    }
}

/// The system-audio device + whether its format comes from the *output* config
/// (Windows loopback) rather than the input config.
fn system_device(host: &Host) -> Option<(Device, bool)> {
    #[cfg(target_os = "windows")]
    {
        // WASAPI loopback: capture the default output device as an input stream.
        host.default_output_device().map(|d| (d, true))
    }
    #[cfg(target_os = "linux")]
    {
        // A PulseAudio/PipeWire monitor source shows up as an input device.
        let monitor = host.input_devices().ok().and_then(|mut devices| {
            devices.find(|d| {
                d.name()
                    .map(|n| n.to_lowercase().contains("monitor"))
                    .unwrap_or(false)
            })
        });
        monitor.map(|d| (d, false))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        // macOS (and others): no native loopback — needs a virtual device.
        let _ = host;
        None
    }
}

/// Open one capture source, resampling its callback data to 48 kHz stereo i16
/// into a shared buffer. `use_output_config` selects the device's output format
/// (Windows loopback) vs its input format.
fn open_source(device: &Device, use_output_config: bool) -> Option<(SourceBuf, Stream)> {
    let supported = if use_output_config {
        device.default_output_config().ok()?
    } else {
        device.default_input_config().ok()?
    };
    let sample_format = supported.sample_format();
    let src_rate = supported.sample_rate().0;
    let src_channels = supported.channels();
    let config: StreamConfig = supported.into();
    let buf: SourceBuf = Arc::new(Mutex::new(VecDeque::new()));

    let stream = match sample_format {
        SampleFormat::F32 => {
            build_typed::<f32>(device, &config, &buf, src_rate, src_channels, |s| s)
        }
        SampleFormat::I16 => {
            build_typed::<i16>(device, &config, &buf, src_rate, src_channels, |s| {
                s as f32 / 32768.0
            })
        }
        SampleFormat::U16 => {
            build_typed::<u16>(device, &config, &buf, src_rate, src_channels, |s| {
                (s as f32 - 32768.0) / 32768.0
            })
        }
        _ => None,
    }?;
    Some((buf, stream))
}

/// Build an input stream for sample type `T`, converting each sample to `f32`
/// with `to_f32`, resampling to the target format, and appending to `buf`.
fn build_typed<T>(
    device: &Device,
    config: &StreamConfig,
    buf: &SourceBuf,
    src_rate: u32,
    src_channels: u16,
    to_f32: fn(T) -> f32,
) -> Option<Stream>
where
    T: cpal::SizedSample + Send + 'static,
{
    let buf = buf.clone();
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let floats: Vec<f32> = data.iter().copied().map(to_f32).collect();
                let resampled = resample_to_target(&floats, src_rate, src_channels);
                if let Ok(mut b) = buf.lock() {
                    b.extend(resampled);
                    while b.len() > MAX_BUFFERED {
                        b.pop_front();
                    }
                }
            },
            on_stream_error,
            None,
        )
        .ok()
}

/// Log (and swallow) a cpal stream error so a glitch never takes down recording.
fn on_stream_error(err: cpal::StreamError) {
    eprintln!("Freally Snipper: audio stream error: {err}");
}

/// Convert interleaved `f32` samples at `src_rate`/`src_channels` to interleaved
/// 48 kHz **stereo** i16. Mono is duplicated to both channels; >2 channels keep
/// the first two. Resampling is per-callback linear interpolation (good enough
/// for screen-recording narration; a stateful resampler is a later refinement).
fn resample_to_target(data: &[f32], src_rate: u32, src_channels: u16) -> Vec<i16> {
    let ch = src_channels as usize;
    if ch == 0 || data.is_empty() {
        return Vec::new();
    }
    let frames = data.len() / ch;
    let stereo = |i: usize| -> (f32, f32) {
        let base = i * ch;
        if ch == 1 {
            (data[base], data[base])
        } else {
            (data[base], data[base + 1])
        }
    };

    let mut out = Vec::with_capacity(frames * 2);
    let mut push = |l: f32, r: f32| {
        out.push((l.clamp(-1.0, 1.0) * 32767.0) as i16);
        out.push((r.clamp(-1.0, 1.0) * 32767.0) as i16);
    };

    if src_rate == TARGET_RATE {
        for i in 0..frames {
            let (l, r) = stereo(i);
            push(l, r);
        }
    } else {
        let ratio = TARGET_RATE as f64 / src_rate as f64;
        let out_frames = (frames as f64 * ratio).round() as usize;
        for i in 0..out_frames {
            let src_pos = i as f64 / ratio;
            let idx = src_pos.floor() as usize;
            let frac = (src_pos - idx as f64) as f32;
            let (l0, r0) = stereo(idx.min(frames.saturating_sub(1)));
            let (l1, r1) = stereo((idx + 1).min(frames.saturating_sub(1)));
            push(l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac);
        }
    }
    out
}
