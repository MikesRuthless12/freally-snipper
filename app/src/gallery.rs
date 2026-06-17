//! Recent-captures gallery thumbnails (P2.2).
//!
//! Decoding and downscaling full-resolution PNG/JPG/BMP captures is far too slow
//! to do on the UI thread, so a background worker decodes each file into a small
//! [`egui::ColorImage`]; the UI only uploads the finished thumbnail as a texture
//! (cheap) and caches it. This keeps the home window smooth no matter how large
//! the captures are.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;

/// Longest edge of a generated thumbnail, in pixels.
const THUMB_MAX: u32 = 160;

/// A decoded thumbnail (or `None` if the file could not be read/decoded).
struct ThumbReady {
    path: PathBuf,
    image: Option<egui::ColorImage>,
}

/// Background thumbnail decoder plus a texture cache, keyed by file path.
pub struct Gallery {
    requests: Sender<PathBuf>,
    ready: Receiver<ThumbReady>,
    textures: HashMap<PathBuf, egui::TextureHandle>,
    requested: HashSet<PathBuf>,
    failed: HashSet<PathBuf>,
}

impl Gallery {
    /// Start the decoder. `ctx` is used to wake the UI when a thumbnail is ready.
    pub fn new(ctx: &egui::Context) -> Self {
        let (req_tx, req_rx) = std::sync::mpsc::channel::<PathBuf>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<ThumbReady>();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("freally-gallery".to_owned())
            .spawn(move || worker(&req_rx, &ready_tx, &ctx));
        if let Err(err) = spawned {
            eprintln!("Freally Snipper: could not start gallery thread: {err}");
        }
        Self {
            requests: req_tx,
            ready: ready_rx,
            textures: HashMap::new(),
            requested: HashSet::new(),
            failed: HashSet::new(),
        }
    }

    /// Upload any thumbnails that finished decoding. Call once per frame before
    /// drawing the strip.
    pub fn pump(&mut self, ctx: &egui::Context) {
        let done: Vec<ThumbReady> = self.ready.try_iter().collect();
        for ThumbReady { path, image } in done {
            match image {
                Some(image) => {
                    let name = format!("thumb:{}", path.display());
                    let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
                    self.textures.insert(path, texture);
                }
                None => {
                    self.failed.insert(path);
                }
            }
        }
    }

    /// The thumbnail texture for `path`, requesting a background decode the first
    /// time it is seen. Returns `None` until the decode finishes (or forever if
    /// the file can't be decoded).
    pub fn thumbnail(&mut self, path: &Path) -> Option<&egui::TextureHandle> {
        if !self.textures.contains_key(path)
            && !self.requested.contains(path)
            && !self.failed.contains(path)
        {
            self.requested.insert(path.to_path_buf());
            if self.requests.send(path.to_path_buf()).is_err() {
                // Worker is gone; don't keep trying.
                self.failed.insert(path.to_path_buf());
            }
        }
        self.textures.get(path)
    }

    /// `true` if the file at `path` could not be decoded (so the UI can draw a
    /// placeholder instead of waiting forever).
    pub fn is_failed(&self, path: &Path) -> bool {
        self.failed.contains(path)
    }
}

fn worker(requests: &Receiver<PathBuf>, ready: &Sender<ThumbReady>, ctx: &egui::Context) {
    while let Ok(path) = requests.recv() {
        let image = load_thumbnail(&path);
        if ready.send(ThumbReady { path, image }).is_err() {
            break; // UI gone
        }
        ctx.request_repaint();
    }
}

/// Decode `path` and downscale it to a thumbnail, or `None` on any failure.
fn load_thumbnail(path: &Path) -> Option<egui::ColorImage> {
    let decoded = image::open(path).ok()?;
    let thumb = decoded.thumbnail(THUMB_MAX, THUMB_MAX).to_rgba8();
    let (w, h) = thumb.dimensions();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        thumb.as_raw(),
    ))
}
