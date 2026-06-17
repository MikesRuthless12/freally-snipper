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

/// Handle to the delivery worker thread.
pub struct Delivery {
    jobs: Sender<Job>,
    results: Receiver<String>,
}

impl Delivery {
    /// Start the worker. `ctx` is used to wake the UI when a delivery finishes.
    pub fn new(ctx: &egui::Context) -> Self {
        let (jobs_tx, jobs_rx) = std::sync::mpsc::channel::<Job>();
        let (results_tx, results_rx) = std::sync::mpsc::channel::<String>();
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

    /// Non-blocking: the most recent finished-delivery status line, if any.
    pub fn poll_status(&self) -> Option<String> {
        // Drain anything queued; the newest message wins.
        self.results.try_iter().last()
    }
}

fn worker(jobs: &Receiver<Job>, results: &Sender<String>, ctx: &egui::Context) {
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

        let message = match output::save_capture(&job.image, &job.folder, job.format) {
            Ok(path) => format!("{prefix}saved {w} × {h} to {}", path.display()),
            Err(err) => format!("{prefix}could not save file: {err}"),
        };

        let _ = results.send(message);
        ctx.request_repaint();
    }
}
