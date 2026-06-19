//! Webcam capture for the recording picture-in-picture (P5.2), via nokhwa
//! (MIT/Apache).
//!
//! A worker thread opens the default camera and keeps the latest frame as raw
//! RGBA8; the recorder samples it each video tick and composites a small PiP into
//! the recorded frame. Best-effort: if no camera is available, recording simply
//! has no PiP. Camera frames are produced as raw bytes (not an `image` type) so
//! this module doesn't couple to a particular `image` crate version — the
//! recorder, which owns the compositing, builds the `RgbaImage`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nokhwa::pixel_format::RgbAFormat;
use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::Camera;

/// The latest decoded webcam frame, as tightly-packed RGBA8.
pub struct WebcamFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

type Shared = Arc<Mutex<Option<WebcamFrame>>>;

/// A running webcam capture. Stops its worker thread on drop.
pub struct Webcam {
    latest: Shared,
    stop: Arc<AtomicBool>,
}

impl Drop for Webcam {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Webcam {
    /// Open the default camera and start delivering frames. Returns `None` if no
    /// camera can be opened (recording then has no PiP).
    pub fn start() -> Option<Self> {
        let latest: Shared = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let latest_worker = latest.clone();
        let stop_worker = stop.clone();
        let (ready_tx, ready_rx) = mpsc::channel::<bool>();
        std::thread::Builder::new()
            .name("freally-webcam".to_owned())
            .spawn(move || worker(&latest_worker, &stop_worker, &ready_tx))
            .ok()?;
        // Wait briefly for the camera to open (or report failure).
        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(true) => Some(Self { latest, stop }),
            _ => {
                stop.store(true, Ordering::Relaxed);
                None
            }
        }
    }

    /// The most recent webcam frame (cloned), or `None` if none has arrived yet.
    pub fn latest(&self) -> Option<WebcamFrame> {
        let guard = self.latest.lock().ok()?;
        guard.as_ref().map(|f| WebcamFrame {
            width: f.width,
            height: f.height,
            rgba: f.rgba.clone(),
        })
    }
}

/// Camera worker: open the default device, then push decoded frames into `latest`
/// until told to stop.
fn worker(latest: &Shared, stop: &AtomicBool, ready: &mpsc::Sender<bool>) {
    let format = RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
    let mut camera = match Camera::new(CameraIndex::Index(0), format) {
        Ok(camera) => camera,
        Err(err) => {
            eprintln!("Freally Snipper: no webcam available: {err}");
            let _ = ready.send(false);
            return;
        }
    };
    if let Err(err) = camera.open_stream() {
        eprintln!("Freally Snipper: couldn't open the webcam stream: {err}");
        let _ = ready.send(false);
        return;
    }
    let _ = ready.send(true);

    while !stop.load(Ordering::Relaxed) {
        match camera
            .frame()
            .and_then(|buf| buf.decode_image::<RgbAFormat>())
        {
            Ok(image) => {
                let frame = WebcamFrame {
                    width: image.width(),
                    height: image.height(),
                    rgba: image.into_raw(),
                };
                if let Ok(mut guard) = latest.lock() {
                    *guard = Some(frame);
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(15)),
        }
    }
    let _ = camera.stop_stream();
}
