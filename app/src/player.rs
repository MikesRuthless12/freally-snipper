//! In-app `.fvid` playback (P5.1): decode the owned container with our own
//! [`freally_video::StreamDecoder`] and show it at the recording's size + fps.
//!
//! A worker thread streams frames one at a time into a small bounded channel; the
//! UI advances a wall-clock playback timer and uploads the frame that is due (so a
//! 4K recording never has to be fully decoded into RAM). Pausing simply stops
//! draining the channel — the worker blocks on backpressure — and looping reopens
//! the stream. This is the **owned decode path** the P5.1 acceptance requires.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use freally_video::{Rational, StreamDecoder};

use crate::export::{ExportFormat, ExportJob};

/// One decoded frame (or end-of-stream) from the worker.
enum FrameMsg {
    Frame {
        index: u64,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
    Ended,
}

/// What the player needs from the host app after a frame.
pub enum PlayerOutcome {
    /// Keep the player open.
    Active,
    /// The user closed the player — restore the home window.
    Close,
    /// The user chose Edit — open this recording in the timeline editor (P6.1).
    Edit(PathBuf),
}

/// Owns the streaming worker thread; stops it on drop.
struct Worker {
    frames: Receiver<FrameMsg>,
    stop: Arc<AtomicBool>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// An in-app `.fvid` player.
pub struct Player {
    path: PathBuf,
    width: u32,
    height: u32,
    fps: Rational,
    frame_count: u32,
    /// Shared with the worker so the Loop toggle takes effect live.
    loop_play: Arc<AtomicBool>,

    worker: Worker,
    texture: Option<egui::TextureHandle>,

    // Wall-clock playback timer.
    cur_index: u64,
    playing: bool,
    start: Instant,
    paused_total: Duration,
    pause_started: Option<Instant>,
    ended: bool,
    /// In-progress / finished export (P5.1), shown in the transport bar.
    export: Option<ExportJob>,
}

impl Player {
    /// Open a `.fvid` for playback: read the header for size/fps, then stream the
    /// frames on a worker thread.
    pub fn open(ctx: &egui::Context, path: PathBuf) -> Result<Self, String> {
        let dec = StreamDecoder::open(&path).map_err(|e| e.to_string())?;
        let (width, height, fps, frame_count) =
            (dec.width(), dec.height(), dec.fps(), dec.frame_count());
        drop(dec);
        if width == 0 || height == 0 || frame_count == 0 || fps.as_f64() <= 0.0 {
            return Err("this recording has no playable frames".to_owned());
        }
        let loop_play = Arc::new(AtomicBool::new(true));
        let worker = spawn_worker(ctx, path.clone(), loop_play.clone());
        Ok(Self {
            path,
            width,
            height,
            fps,
            frame_count,
            loop_play,
            worker,
            texture: None,
            cur_index: 0,
            playing: true,
            start: Instant::now(),
            paused_total: Duration::ZERO,
            pause_started: None,
            ended: false,
            export: None,
        })
    }

    fn elapsed(&self) -> Duration {
        let pausing = self.pause_started.map(|p| p.elapsed()).unwrap_or_default();
        self.start
            .elapsed()
            .saturating_sub(self.paused_total + pausing)
    }

    fn fps_f64(&self) -> f64 {
        self.fps.as_f64().max(1.0)
    }

    fn pause(&mut self) {
        if self.pause_started.is_none() {
            self.pause_started = Some(Instant::now());
            self.playing = false;
        }
    }

    fn resume(&mut self) {
        if let Some(p) = self.pause_started.take() {
            self.paused_total += p.elapsed();
        }
        self.playing = true;
    }

    /// Restart playback from the beginning (respawns the stream).
    fn restart(&mut self, ctx: &egui::Context) {
        self.worker = spawn_worker(ctx, self.path.clone(), self.loop_play.clone());
        self.cur_index = 0;
        self.start = Instant::now();
        self.paused_total = Duration::ZERO;
        self.pause_started = None;
        self.ended = false;
        self.playing = true;
    }

    /// Pull any frames now due, upload the latest, and draw the video + transport.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> PlayerOutcome {
        let ctx = ui.ctx().clone();
        let mut outcome = PlayerOutcome::Active;

        if self.playing && !self.ended {
            self.pump_frames(&ctx);
            ctx.request_repaint();
        }
        if let Some(job) = &mut self.export {
            job.poll();
            if job.done.is_none() {
                ctx.request_repaint();
            }
        }

        // Collected here so the (deeply nested) Export menu closure doesn't have to
        // mutate `self.export` directly.
        let mut export_request: Option<(ExportFormat, PathBuf)> = None;
        let exporting = self.export.as_ref().is_some_and(|j| j.done.is_none());
        let export_src = self.path.clone();

        egui::Panel::bottom("player_controls").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let play_label = if self.ended {
                    "↻ Replay"
                } else if self.playing {
                    "⏸ Pause"
                } else {
                    "▶ Play"
                };
                if ui.button(play_label).clicked() {
                    if self.ended {
                        self.restart(&ctx);
                    } else if self.playing {
                        self.pause();
                    } else {
                        self.resume();
                    }
                }
                if ui
                    .button("⟲ Restart")
                    .on_hover_text("Play from the start")
                    .clicked()
                {
                    self.restart(&ctx);
                }
                let mut looping = self.loop_play.load(Ordering::Relaxed);
                if ui.checkbox(&mut looping, "Loop").changed() {
                    self.loop_play.store(looping, Ordering::Relaxed);
                    // Leaving a finished, now-looping clip should resume.
                    if looping && self.ended {
                        self.restart(&ctx);
                    }
                }

                let pos = if self.frame_count == 0 {
                    0
                } else {
                    self.cur_index.saturating_sub(1) % self.frame_count as u64
                };
                ui.label(format!(
                    "{} / {}",
                    clock(pos as f64 / self.fps_f64()),
                    clock(self.frame_count as f64 / self.fps_f64())
                ));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        outcome = PlayerOutcome::Close;
                    }
                    if ui
                        .button("✎ Edit")
                        .on_hover_text("Open this recording in the timeline editor")
                        .clicked()
                    {
                        outcome = PlayerOutcome::Edit(self.path.clone());
                    }
                    ui.add_enabled_ui(!exporting, |ui| {
                        ui.menu_button("Export ▾", |ui| {
                            for fmt in ExportFormat::ALL {
                                if ui.button(fmt.label()).clicked() {
                                    if let Some(dst) = save_dialog(&export_src, fmt) {
                                        export_request = Some((fmt, dst));
                                    }
                                    ui.close();
                                }
                            }
                        })
                        .response
                        .on_hover_text(
                            "WebM/MP4 use ffmpeg (fetched on first use); GIF is built-in",
                        );
                    });
                    ui.label(
                        egui::RichText::new(format!(
                            "{} × {} · {:.0} fps",
                            self.width,
                            self.height,
                            self.fps_f64()
                        ))
                        .weak()
                        .small(),
                    );
                });
            });
            if let Some(job) = &self.export {
                export_status_row(ui, job);
            }
            ui.add_space(4.0);
        });

        if let Some((format, dst)) = export_request {
            self.export = Some(ExportJob::start(&ctx, self.path.clone(), dst, format));
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let avail = ui.available_size();
            match &self.texture {
                Some(tex) => {
                    let fit = fit_within([self.width as f32, self.height as f32], avail);
                    ui.centered_and_justified(|ui| {
                        ui.add(egui::Image::from_texture(egui::load::SizedTexture::new(
                            tex.id(),
                            fit,
                        )));
                    });
                }
                None => {
                    ui.centered_and_justified(|ui| {
                        ui.spinner();
                    });
                }
            }
        });

        if ui.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            outcome = PlayerOutcome::Close;
        }
        outcome
    }

    /// Drain frames up to the wall-clock target, uploading only the latest (older
    /// frames are dropped if the UI fell behind, so playback stays in time).
    fn pump_frames(&mut self, ctx: &egui::Context) {
        let target = (self.elapsed().as_secs_f64() * self.fps_f64()).floor() as u64 + 1;
        let mut latest: Option<(u32, u32, Vec<u8>)> = None;
        while self.cur_index < target {
            match self.worker.frames.try_recv() {
                Ok(FrameMsg::Frame {
                    index,
                    width,
                    height,
                    pixels,
                }) => {
                    latest = Some((width, height, pixels));
                    self.cur_index = index + 1;
                }
                Ok(FrameMsg::Ended) => {
                    self.ended = true;
                    self.playing = false;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.ended = true;
                    break;
                }
            }
        }
        if let Some((w, h, px)) = latest {
            let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
            match &mut self.texture {
                Some(tex) => tex.set(color, egui::TextureOptions::LINEAR),
                None => {
                    self.texture =
                        Some(ctx.load_texture("fvid_player", color, egui::TextureOptions::LINEAR));
                }
            }
        }
    }
}

/// Spawn the streaming worker for `path`.
fn spawn_worker(ctx: &egui::Context, path: PathBuf, loop_play: Arc<AtomicBool>) -> Worker {
    let (tx, rx) = sync_channel::<FrameMsg>(4);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = stop.clone();
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new()
        .name("freally-player".to_owned())
        .spawn(move || worker_run(&path, &loop_play, &tx, &stop_worker, &ctx));
    if spawned.is_err() {
        // The receiver will simply never get frames; the UI shows the spinner.
        eprintln!("Freally Snipper: could not start the player thread");
    }
    Worker { frames: rx, stop }
}

/// Stream frames from the `.fvid` until told to stop, looping while `loop_play`.
fn worker_run(
    path: &Path,
    loop_play: &AtomicBool,
    tx: &SyncSender<FrameMsg>,
    stop: &AtomicBool,
    ctx: &egui::Context,
) {
    let mut index = 0u64;
    loop {
        let mut dec = match StreamDecoder::open(path) {
            Ok(dec) => dec,
            Err(_) => {
                let _ = tx.send(FrameMsg::Ended);
                return;
            }
        };
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            match dec.next_frame() {
                Ok(Some(frame)) => {
                    let msg = FrameMsg::Frame {
                        index,
                        width: frame.width,
                        height: frame.height,
                        pixels: frame.pixels,
                    };
                    // Blocks (backpressure) while paused; errors once the UI drops.
                    if tx.send(msg).is_err() {
                        return;
                    }
                    index += 1;
                    ctx.request_repaint();
                }
                Ok(None) => break, // end of stream
                Err(_) => {
                    let _ = tx.send(FrameMsg::Ended);
                    return;
                }
            }
        }
        if !loop_play.load(Ordering::Relaxed) {
            let _ = tx.send(FrameMsg::Ended);
            return;
        }
        // Loop: reopen and keep a globally increasing index so the clock matches.
    }
}

/// A native save dialog for an export, defaulting to the recording's name + the
/// format's extension, in the recording's folder.
fn save_dialog(src: &Path, format: ExportFormat) -> Option<PathBuf> {
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "recording".to_owned());
    let mut dialog = rfd::FileDialog::new()
        .set_file_name(format!("{stem}.{}", format.ext()))
        .add_filter(format.label(), &[format.ext()]);
    if let Some(parent) = src.parent().filter(|p| p.is_dir()) {
        dialog = dialog.set_directory(parent);
    }
    dialog.save_file()
}

/// Draw the export progress / result line under the transport controls.
fn export_status_row(ui: &mut egui::Ui, job: &ExportJob) {
    match &job.done {
        None => {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Exporting {} … {:.0}%",
                    job.format.label(),
                    job.progress * 100.0
                ));
                ui.spinner();
            });
        }
        Some(Ok(path)) => {
            ui.label(egui::RichText::new(format!("Exported to {}", path.display())).weak());
        }
        Some(Err(err)) => {
            ui.label(
                egui::RichText::new(format!("Export failed: {err}"))
                    .color(egui::Color32::from_rgb(220, 80, 80)),
            );
        }
    }
}

/// `M:SS` clock string for the transport readout.
fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Scale `size` to fit within `avail`, preserving aspect ratio (never upscaling
/// past the available area).
fn fit_within(size: [f32; 2], avail: egui::Vec2) -> egui::Vec2 {
    let (w, h) = (size[0].max(1.0), size[1].max(1.0));
    let scale = (avail.x / w).min(avail.y / h).max(0.0);
    egui::vec2(w * scale, h * scale)
}
