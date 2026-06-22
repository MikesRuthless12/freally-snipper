//! Audio output for the timeline preview (P6.1 / B9) via cpal.
//!
//! The timeline mixer ([`freally_timeline::compose_audio`]) produces the whole
//! edit's audio as **48 kHz stereo i16**; this module plays it back in sync with
//! the video preview. A cpal output [`Stream`] is **`!Send`** and must stay on the
//! thread that built it, so the stream lives on a dedicated **audio thread** and
//! the app holds only `Send` control handles (a shared [`PlayState`] + a shutdown
//! flag) — the same "streams live off the app struct" shape the recorder uses.
//!
//! Playback reads from a preloaded mix buffer at a fractional cursor (so any output
//! device rate works), advancing on the audio hardware clock. The video playhead
//! runs on the wall clock; both start together, which keeps a short preview in
//! sync without a tight A/V lock.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

/// The mix buffer's format (matches the timeline / recorder / codec).
const SRC_RATE: u32 = 48_000;
const SRC_CHANNELS: usize = 2;

/// Playback state shared with the audio callback.
#[derive(Default)]
struct PlayState {
    /// Interleaved stereo i16 @ 48 kHz — the whole edit's mixed audio.
    mix: Arc<Vec<i16>>,
    /// Per-channel sample position (fractional, to allow rate conversion).
    pos: f64,
    /// Whether playback is currently running.
    playing: bool,
}

/// Plays the timeline's mixed audio on the default output device. Best-effort:
/// [`AudioPreview::new`] returns `None` when no device/stream is available, and the
/// editor simply previews silently (like the P5 player).
pub struct AudioPreview {
    state: Arc<Mutex<PlayState>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl AudioPreview {
    /// Open the default output device on a dedicated thread. `None` if unavailable.
    pub fn new() -> Option<Self> {
        let state: Arc<Mutex<PlayState>> = Arc::new(Mutex::new(PlayState::default()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();

        let handle = {
            let state = state.clone();
            let shutdown = shutdown.clone();
            std::thread::Builder::new()
                .name("freally-audio-out".to_owned())
                .spawn(move || audio_thread(state, &shutdown, &ready_tx))
                .ok()?
        };

        // Wait for the thread to report whether the stream opened.
        match ready_rx.recv() {
            Ok(true) => Some(Self {
                state,
                shutdown,
                handle: Some(handle),
            }),
            _ => {
                shutdown.store(true, Ordering::Relaxed);
                let _ = handle.join();
                None
            }
        }
    }

    /// Replace the mix buffer (call after an edit changes the audio).
    pub fn set_mix(&self, mix: Arc<Vec<i16>>) {
        if let Ok(mut st) = self.state.lock() {
            st.mix = mix;
        }
    }

    /// Start playback from per-channel sample `start_sample`.
    pub fn play_from(&self, start_sample: u64) {
        if let Ok(mut st) = self.state.lock() {
            st.pos = start_sample as f64;
            st.playing = true;
        }
    }

    /// Pause playback (the cursor stays put).
    pub fn stop(&self) {
        if let Ok(mut st) = self.state.lock() {
            st.playing = false;
        }
    }
}

impl Drop for AudioPreview {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Build the output stream, report success over `ready_tx`, then keep the (`!Send`)
/// stream alive on this thread until shutdown.
fn audio_thread(
    state: Arc<Mutex<PlayState>>,
    shutdown: &AtomicBool,
    ready_tx: &mpsc::Sender<bool>,
) {
    let stream = match build_stream(state) {
        Some(s) if s.play().is_ok() => s,
        _ => {
            let _ = ready_tx.send(false);
            return;
        }
    };
    let _ = ready_tx.send(true);
    while !shutdown.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(stream); // stops audio
}

/// Open the default output device and build a playback stream over `state`.
fn build_stream(state: Arc<Mutex<PlayState>>) -> Option<Stream> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let supported = device.default_output_config().ok()?;
    let sample_format = supported.sample_format();
    let out_rate = supported.sample_rate().0;
    let out_channels = supported.channels() as usize;
    let config: StreamConfig = supported.into();

    match sample_format {
        SampleFormat::F32 => {
            build_typed::<f32>(&device, &config, state, out_rate, out_channels, |s| {
                s as f32 / 32768.0
            })
        }
        SampleFormat::I16 => {
            build_typed::<i16>(&device, &config, state, out_rate, out_channels, |s| s)
        }
        SampleFormat::U16 => {
            build_typed::<u16>(&device, &config, state, out_rate, out_channels, |s| {
                (s as i32 + 32768) as u16
            })
        }
        _ => None,
    }
}

/// Build an output stream for sample type `T`, converting each i16 mix sample with
/// `from_i16`, mapping stereo → the device's channel count, and resampling 48 kHz →
/// the device rate by stepping a fractional cursor.
fn build_typed<T>(
    device: &Device,
    config: &StreamConfig,
    state: Arc<Mutex<PlayState>>,
    out_rate: u32,
    out_channels: usize,
    from_i16: fn(i16) -> T,
) -> Option<Stream>
where
    T: cpal::SizedSample + Send + 'static,
{
    let step = SRC_RATE as f64 / out_rate.max(1) as f64;
    let silence = from_i16(0);
    let out_channels = out_channels.max(1);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let Ok(mut st) = state.lock() else {
                    data.fill(silence);
                    return;
                };
                if !st.playing {
                    data.fill(silence);
                    return;
                }
                let mix = st.mix.clone();
                let frames = mix.len() / SRC_CHANNELS;
                let mut pos = st.pos;
                for frame in data.chunks_mut(out_channels) {
                    let i = pos as usize;
                    if i >= frames {
                        st.playing = false; // reached the end
                        for s in frame.iter_mut() {
                            *s = silence;
                        }
                        continue;
                    }
                    let (l, r) = (mix[i * 2], mix[i * 2 + 1]);
                    for (c, s) in frame.iter_mut().enumerate() {
                        *s = match c {
                            0 => from_i16(l),
                            1 => from_i16(r),
                            _ => silence,
                        };
                    }
                    pos += step;
                }
                st.pos = pos;
            },
            on_error,
            None,
        )
        .ok()
}

/// Log (and swallow) a cpal output error so a glitch never crashes the editor.
fn on_error(err: cpal::StreamError) {
    eprintln!("Freally Snipper: audio output error: {err}");
}
