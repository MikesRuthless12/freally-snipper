//! Background delivery of a finished capture: clipboard copy + file save, run on
//! a persistent worker thread so committing a capture feels instant (PNG
//! encoding a full-desktop image is far too slow to do on the UI thread).
//!
//! The worker owns its [`arboard::Clipboard`] for its whole life: on Linux/X11
//! the clipboard is served by the owning context, so a short-lived clipboard
//! would lose the contents the moment it dropped.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;
use freally_capture::image::RgbaImage;

use crate::output;
use crate::settings::ImageFormat;

/// One capture to deliver (copy to clipboard + save to disk).
struct Job {
    image: RgbaImage,
    folder: PathBuf,
    format: ImageFormat,
}

/// The outcome of one delivery: a human-readable status line plus, on a
/// successful save, the file path (so the home-window gallery can record it).
pub struct DeliveryResult {
    pub message: String,
    pub saved_path: Option<PathBuf>,
}

/// Handle to the delivery worker thread.
pub struct Delivery {
    jobs: Sender<Job>,
    results: Receiver<DeliveryResult>,
}

impl Delivery {
    /// Start the worker. `ctx` is used to wake the UI when a delivery finishes.
    pub fn new(ctx: &egui::Context) -> Self {
        let (jobs_tx, jobs_rx) = std::sync::mpsc::channel::<Job>();
        let (results_tx, results_rx) = std::sync::mpsc::channel::<DeliveryResult>();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("freally-delivery".to_owned())
            .spawn(move || worker(&jobs_rx, &results_tx, &ctx));
        if let Err(err) = spawned {
            eprintln!("Freally Snipper: could not start delivery thread: {err}");
        }
        Self {
            jobs: jobs_tx,
            results: results_rx,
        }
    }

    /// Queue a capture for clipboard + save. Returns immediately.
    pub fn deliver(&self, image: RgbaImage, folder: PathBuf, format: ImageFormat) {
        if self
            .jobs
            .send(Job {
                image,
                folder,
                format,
            })
            .is_err()
        {
            eprintln!("Freally Snipper: delivery worker is gone; capture not saved");
        }
    }

    /// Non-blocking: drain every finished delivery since the last call. All
    /// results are returned (not just the newest) so no saved path is lost from
    /// the gallery when two captures finish within one frame.
    pub fn poll(&self) -> Vec<DeliveryResult> {
        self.results.try_iter().collect()
    }
}

fn worker(jobs: &Receiver<Job>, results: &Sender<DeliveryResult>, ctx: &egui::Context) {
    // Created once and kept alive for the thread's lifetime (Linux clipboard
    // serving depends on this instance staying alive).
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(clipboard) => Some(clipboard),
        Err(err) => {
            eprintln!("Freally Snipper: clipboard unavailable: {err}");
            None
        }
    };

    while let Ok(job) = jobs.recv() {
        let (w, h) = (job.image.width(), job.image.height());

        let clipboard_ok = match clipboard.as_mut() {
            Some(cb) => match output::set_clipboard_image(cb, &job.image) {
                Ok(()) => true,
                Err(err) => {
                    eprintln!("Freally Snipper: clipboard copy failed: {err}");
                    false
                }
            },
            None => false,
        };
        let prefix = if clipboard_ok {
            "Copied to clipboard · "
        } else {
            "Clipboard unavailable · "
        };

        let result = match output::save_capture(&job.image, &job.folder, job.format) {
            Ok(path) => DeliveryResult {
                message: format!("{prefix}saved {w} × {h} to {}", path.display()),
                saved_path: Some(path),
            },
            Err(err) => DeliveryResult {
                message: format!("{prefix}could not save file: {err}"),
                saved_path: None,
            },
        };

        let _ = results.send(result);
        ctx.request_repaint();
    }
}
