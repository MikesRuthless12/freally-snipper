//! Screen recording (P5.1): a worker thread grabs frames in a timed loop and
//! streams them straight to a `.fvid` file via [`freally_video::StreamEncoder`],
//! so an open-ended (and possibly 4K) recording never holds all frames in RAM.
//!
//! The loop is **real-time-accurate**: each tick it works out how many frame
//! slots have elapsed on the wall clock and emits exactly that many frames,
//! duplicating the latest grab to fill any slots a slow capture/encode missed.
//! Duplicates are nearly free in the codec (an identical inter-frame is a few
//! bytes), so playback length always matches the real elapsed time even when the
//! owned lossless encoder can't sustain the target rate at 4K.
//!
//! **Attach-to-window** mode follows a chosen window via
//! [`freally_capture::TrackedWindow`]: each tick re-reads the window's live bounds
//! (so it tracks moves/resizes) and scales the grab to the fixed recording size.
//!
//! Audio (P5.2) is layered on later through [`freally_video::StreamEncoder`]'s
//! `push_audio`; this module currently records video only.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui;
use freally_capture::image::{imageops, RgbaImage};
use freally_capture::{capture_region, Rect as VRect, TrackedWindow};
use freally_video::{Rational, StreamEncoder};

use crate::audio::{AudioCapture, AudioConfig};
use crate::webcam::{Webcam, WebcamFrame};

/// Settle time before the first grab, so the morph from the selection overlay to
/// the small control bar completes (and the overlay leaves the recorded area)
/// before recording starts — so it's never in the shot.
const STARTUP_SETTLE: Duration = Duration::from_millis(200);

/// What to record.
pub enum RecordTarget {
    /// A fixed virtual-desktop region (also used for full screen = the whole
    /// virtual desktop, or a single monitor).
    Region(VRect),
    /// Attach to a specific window and follow it. `initial` is its bounds at the
    /// start (the fixed output size, and the fallback if tracking ever drops out).
    Window { id: u32, initial: VRect },
}

/// Everything the recorder worker needs to run.
pub struct RecordConfig {
    pub target: RecordTarget,
    /// Target frames per second (clamped to at least 1).
    pub fps: u32,
    /// Final `.fvid` path (written atomically via a sibling `.part` file).
    pub output: PathBuf,
    /// Which audio sources to capture (P5.2). Best-effort: a source that can't
    /// open is skipped, and with none the recording is video-only.
    pub audio: AudioConfig,
    /// Overlay the webcam as a picture-in-picture (P5.2). Best-effort.
    pub webcam: bool,
}

/// Commands the UI sends to the recording worker.
enum RecordCommand {
    Pause,
    Resume,
    Stop,
}

/// A snapshot of the recording's progress, polled by the UI each frame.
pub struct RecordStatus {
    /// Elapsed recording time (excludes paused spans).
    pub elapsed: Duration,
    /// Whether the recording is currently paused.
    pub paused: bool,
    /// `Some` once the worker has stopped: the saved path, or an error message.
    pub finished: Option<std::result::Result<PathBuf, String>>,
}

/// Handle to a running recording. Dropping it **stops the worker and waits** for
/// the `.fvid` to be finalized, so a recording is never lost if the app exits
/// mid-record. For the normal path, call [`stop`](Self::stop) and poll until
/// [`RecordStatus::finished`] is set.
pub struct Recorder {
    commands: Sender<RecordCommand>,
    status: Receiver<RecordStatus>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Recorder {
    /// Start recording on a background thread. `ctx` wakes the UI as progress and
    /// the final result arrive.
    pub fn start(ctx: &egui::Context, config: RecordConfig) -> Self {
        let (commands_tx, commands_rx) = mpsc::channel();
        let (status_tx, status_rx) = mpsc::channel();
        // A spare sender so a spawn failure can still be reported (the worker moves
        // the original into its thread).
        let err_status = status_tx.clone();
        let ctx = ctx.clone();
        let handle = match std::thread::Builder::new()
            .name("freally-recorder".to_owned())
            .spawn(move || run(config, &commands_rx, &status_tx, &ctx))
        {
            Ok(handle) => Some(handle),
            Err(err) => {
                let _ = err_status.send(finished_err(format!(
                    "Couldn't start the recording thread: {err}"
                )));
                None
            }
        };
        Self {
            commands: commands_tx,
            status: status_rx,
            handle,
        }
    }

    /// Pause capturing (recorded time stops advancing).
    pub fn pause(&self) {
        let _ = self.commands.send(RecordCommand::Pause);
    }

    /// Resume after a pause.
    pub fn resume(&self) {
        let _ = self.commands.send(RecordCommand::Resume);
    }

    /// Stop recording; the worker finalizes the file and reports the saved path
    /// through a final [`RecordStatus`].
    pub fn stop(&self) {
        let _ = self.commands.send(RecordCommand::Stop);
    }

    /// Drain all status updates since the last poll (the last one is the newest).
    pub fn poll(&self) -> Vec<RecordStatus> {
        self.status.try_iter().collect()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // Finalize the recording even if the app exits mid-record: tell the worker
        // to stop, then wait for it to flush + rename the .fvid into place.
        let _ = self.commands.send(RecordCommand::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// The recording loop. Runs until a `Stop` command (or a fatal error), then
/// finalizes the `.fvid` and reports the result.
fn run(
    config: RecordConfig,
    commands: &Receiver<RecordCommand>,
    status: &Sender<RecordStatus>,
    ctx: &egui::Context,
) {
    let (out_w, out_h) = match &config.target {
        RecordTarget::Region(r) => (r.width, r.height),
        RecordTarget::Window { initial, .. } => (initial.width, initial.height),
    };
    if out_w == 0 || out_h == 0 {
        let _ = status.send(finished_err("Recording area is empty.".to_owned()));
        return;
    }

    let fps = config.fps.max(1);
    let temp = temp_path(&config.output);

    // Best-effort audio (P5.2): if a source opens, record A/V; else video-only.
    let audio = if config.audio.any() {
        AudioCapture::start(config.audio)
    } else {
        None
    };

    // Best-effort webcam PiP (P5.2): overlaid onto each frame if a camera opens.
    let webcam = if config.webcam { Webcam::start() } else { None };

    let encoder_result = match &audio {
        Some(cap) => StreamEncoder::create_with_audio(
            &temp,
            out_w,
            out_h,
            Rational::new(fps, 1),
            cap.sample_rate(),
            cap.channels(),
        ),
        None => StreamEncoder::create(&temp, out_w, out_h, Rational::new(fps, 1)),
    };
    let mut encoder = match encoder_result {
        Ok(enc) => enc,
        Err(err) => {
            let _ = status.send(finished_err(format!("Couldn't start recording: {err}")));
            return;
        }
    };

    // Window mode: hold the window handle to follow it; fall back to the initial
    // bounds if it can't be found or temporarily can't be read.
    let tracked = match &config.target {
        RecordTarget::Window { id, .. } => TrackedWindow::find(*id).ok().flatten(),
        RecordTarget::Region(_) => None,
    };
    let mut last_bounds = match &config.target {
        RecordTarget::Region(r) => *r,
        RecordTarget::Window { initial, .. } => *initial,
    };

    // Let the overlay window finish morphing into the small control bar (and out of
    // the recorded area) before the first grab, so it's never in the shot.
    std::thread::sleep(STARTUP_SETTLE);
    // Drop audio captured during the settle so A/V start at the same instant.
    if let Some(cap) = &audio {
        let _ = cap.drain();
    }

    let frame_interval = Duration::from_secs_f64(1.0 / fps as f64);
    let start = Instant::now();
    let mut paused_total = Duration::ZERO;
    let mut pause_started: Option<Instant> = None;
    let mut pushed: u64 = 0;
    let mut last_frame: Option<RgbaImage> = None;

    loop {
        let tick_start = Instant::now();

        // ---- drain commands ----
        let mut stop = false;
        while let Ok(cmd) = commands.try_recv() {
            match cmd {
                RecordCommand::Pause => {
                    if pause_started.is_none() {
                        pause_started = Some(Instant::now());
                    }
                }
                RecordCommand::Resume => {
                    if let Some(p) = pause_started.take() {
                        paused_total += p.elapsed();
                    }
                }
                RecordCommand::Stop => stop = true,
            }
        }
        if stop {
            break;
        }

        // ---- paused: report and idle, don't advance the timeline ----
        if let Some(p) = pause_started {
            // Discard audio captured while paused so it isn't muxed in.
            if let Some(cap) = &audio {
                let _ = cap.drain();
            }
            let _ = status.send(RecordStatus {
                elapsed: start.elapsed().saturating_sub(paused_total + p.elapsed()),
                paused: true,
                finished: None,
            });
            ctx.request_repaint();
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }

        // ---- choose the capture rect (follow the window if attached) ----
        let rect = match &tracked {
            Some(tw) => match tw.bounds() {
                Some(b) => {
                    last_bounds = b;
                    b
                }
                None => last_bounds,
            },
            None => last_bounds,
        };

        // ---- grab + normalize to the fixed output size ----
        let frame = match capture_region(rect) {
            Ok(img) => {
                if img.width() == out_w && img.height() == out_h {
                    Some(img)
                } else {
                    Some(imageops::resize(
                        &img,
                        out_w,
                        out_h,
                        imageops::FilterType::Triangle,
                    ))
                }
            }
            // Transient failure (e.g. the window momentarily can't be grabbed):
            // reuse the previous frame so the timeline stays continuous.
            Err(_) => last_frame.clone(),
        };
        let Some(mut frame) = frame else {
            // No frame captured yet and the first grab failed — wait and retry.
            std::thread::sleep(frame_interval);
            continue;
        };

        // ---- composite the webcam PiP (P5.2) onto the frame ----
        if let Some(cam) = &webcam {
            if let Some(wf) = cam.latest() {
                composite_pip(&mut frame, &wf, out_w, out_h);
            }
        }

        // ---- emit enough frames to match the wall clock (duplicates are cheap) ----
        let elapsed = start.elapsed().saturating_sub(paused_total);
        let target_count = (elapsed.as_secs_f64() * fps as f64).floor() as u64 + 1;
        while pushed < target_count {
            if let Err(err) = encoder.push_rgba(frame.as_raw()) {
                let _ = status.send(finished_err(format!("Recording write failed: {err}")));
                return;
            }
            pushed += 1;
        }
        last_frame = Some(frame);

        // ---- append any captured audio to the track (P5.2) ----
        if let Some(cap) = &audio {
            let samples = cap.drain();
            if !samples.is_empty() {
                if let Err(err) = encoder.push_audio(&samples) {
                    let _ = status.send(finished_err(format!("Recording audio failed: {err}")));
                    return;
                }
            }
        }

        let _ = status.send(RecordStatus {
            elapsed,
            paused: false,
            finished: None,
        });
        ctx.request_repaint();

        // ---- pace to the target frame rate ----
        let work = tick_start.elapsed();
        if work < frame_interval {
            std::thread::sleep(frame_interval - work);
        }
    }

    // ---- finalize: flush remaining audio, patch the header, move into place ----
    if let Some(cap) = &audio {
        let samples = cap.drain();
        if !samples.is_empty() {
            let _ = encoder.push_audio(&samples);
        }
    }
    let elapsed = start.elapsed().saturating_sub(paused_total);
    let result = match encoder.finish() {
        Ok(writer) => {
            // Drop the file handle before renaming (Windows won't move an open file).
            drop(writer);
            std::fs::rename(&temp, &config.output)
                .map(|()| config.output.clone())
                .map_err(|err| format!("Couldn't save the recording: {err}"))
        }
        Err(err) => Err(format!("Couldn't finalize the recording: {err}")),
    };
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    let _ = status.send(RecordStatus {
        elapsed,
        paused: false,
        finished: Some(result),
    });
    ctx.request_repaint();
}

/// A sibling `<output>.part` path the recording streams into, renamed onto the
/// final path only once it is complete (so a half-written file never appears).
fn temp_path(output: &Path) -> PathBuf {
    let mut name: OsString = output.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Composite the webcam frame as a small picture-in-picture in the bottom-right
/// corner of `frame` (P5.2), preserving the camera's aspect ratio.
fn composite_pip(frame: &mut RgbaImage, cam: &WebcamFrame, out_w: u32, out_h: u32) {
    let Some(source) = RgbaImage::from_raw(cam.width, cam.height, cam.rgba.clone()) else {
        return;
    };
    if source.width() == 0 || source.height() == 0 {
        return;
    }
    // PiP about a fifth of the frame width, aspect-preserving, clamped to the frame.
    let pip_w = (out_w / 5).clamp(80, out_w);
    let pip_h =
        ((pip_w as u64 * source.height() as u64 / source.width() as u64) as u32).clamp(1, out_h);
    let pip = imageops::resize(&source, pip_w, pip_h, imageops::FilterType::Triangle);
    let margin = (out_w / 50).max(8);
    let x = out_w.saturating_sub(pip_w + margin) as i64;
    let y = out_h.saturating_sub(pip_h + margin) as i64;
    imageops::overlay(frame, &pip, x, y);
}

/// Build a terminal status carrying an error message.
fn finished_err(message: String) -> RecordStatus {
    RecordStatus {
        elapsed: Duration::ZERO,
        paused: false,
        finished: Some(Err(message)),
    }
}
