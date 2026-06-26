//! The Phase 6 timeline editor screen (P6.1).
//!
//! Opens a saved `.fvid` recording as a one-clip [`Timeline`], then lets the user
//! scrub (frame-accurate), play (**with audio**), frame-step, split, ripple-delete,
//! **drag clips to move them**, **drag their edges to trim**, and adjust per-clip
//! **opacity / gain / fades** on a multi-track strip — with a live WYSIWYG preview
//! composited by the pure [`freally_timeline`] compositor. Export renders the whole
//! timeline to a temp `.fvid` (on a worker thread) and hands it to the existing
//! [`ExportJob`] (GIF / WebM / MP4), so "edits reflect in preview **and** export".
//!
//! Mirrors [`crate::player`]: the host app morphs its single window into a
//! decorated editor window, calls [`TimelineEditor::ui`] each frame, and restores
//! the home window when it returns [`TimelineOutcome::Close`]. Audio plays through
//! [`crate::audio_out::AudioPreview`]; the video playhead runs on the wall clock,
//! and both start together (a short preview stays in sync without a tight A/V lock).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui;
use freally_timeline::{
    compose_audio, compose_frame, Clip, MediaId, Timecode, Timeline, TrackKind,
};
use freally_video::{Rational, StreamEncoder};

use crate::audio_out::AudioPreview;
use crate::export::{ExportFormat, ExportJob};
use crate::fvid_reader::{AppProvider, FvidReader};

/// Strip geometry (points).
const ROW_H: f32 = 26.0;
const GAP: f32 = 3.0;
const PAD: f32 = 4.0;
/// How close to a clip edge (points) counts as grabbing it for a trim.
const EDGE_GRAB: f32 = 6.0;
/// Cap the precomputed audio-preview mix at 10 minutes (per-channel samples) so a
/// very long edit can't allocate an unbounded buffer; audio past it previews silent.
const MAX_PREVIEW_SAMPLES: u64 = 10 * 60 * 48_000;

/// What the editor needs from the host app after a frame.
pub enum TimelineOutcome {
    /// Keep the editor open.
    Active,
    /// The user closed the editor — restore the home window.
    Close,
}

/// An in-progress pointer drag on the strip (spans frames).
#[derive(Clone, Copy)]
enum DragOp {
    /// Move the playhead (drag on empty space).
    Scrub,
    /// Move a clip; `grab_offset` keeps the grab point under the cursor.
    Move {
        track: usize,
        clip: usize,
        grab_offset: i64,
    },
    /// Trim a clip's head (left edge).
    TrimHead { track: usize, clip: usize },
    /// Trim a clip's tail (right edge).
    TrimTail { track: usize, clip: usize },
}

/// The running export, a two-phase pipeline: render the timeline to a temp
/// `.fvid`, then encode that to the chosen format with [`ExportJob`].
enum Exporting {
    Rendering {
        job: RenderJob,
        format: ExportFormat,
        dst: PathBuf,
        temp: PathBuf,
    },
    Encoding {
        job: ExportJob,
        temp: PathBuf,
    },
}

/// An in-app timeline editor over one or more `.fvid` sources.
pub struct TimelineEditor {
    timeline: Timeline,
    provider: AppProvider,
    /// `(id, path)` for every source, so a separate provider can be rebuilt for the
    /// export worker thread (readers hold a file handle and aren't shared).
    sources: Vec<(MediaId, PathBuf)>,
    fps: Rational,

    // Transport.
    playhead: u64,
    playing: bool,
    play_origin: u64,
    play_start: Instant,

    // Audio preview (best-effort; `None` if no output device).
    audio: Option<AudioPreview>,
    /// The mix is stale and must be recomputed before the next play.
    audio_dirty: bool,

    // Preview.
    texture: Option<egui::TextureHandle>,
    rendered: Option<u64>,
    dirty: bool,

    // Editing.
    active_track: usize,
    selected: Option<(usize, usize)>,
    drag: Option<DragOp>,

    export: Option<Exporting>,
    status: Option<String>,
}

impl TimelineEditor {
    /// Open a `.fvid` recording as a single full-length clip on one video track.
    pub fn from_recording(ctx: &egui::Context, path: PathBuf) -> Result<Self, String> {
        let reader = FvidReader::open(&path)?;
        let (w, h, fps, count) = (
            reader.width(),
            reader.height(),
            reader.fps(),
            reader.frame_count(),
        );
        if w == 0 || h == 0 || count == 0 || fps.as_f64() <= 0.0 {
            return Err("this recording has no editable frames".to_owned());
        }
        let id = MediaId(0);
        let mut provider = AppProvider::new();
        provider.insert(id, reader);

        let mut timeline = Timeline::new(w, h, fps);
        timeline.register_media(id, count as u64);
        let v = timeline.push_track(TrackKind::Video, "V1");
        timeline.add_clip(v, Clip::media(id, 0, count as u64, 0));

        let mut editor = Self {
            timeline,
            provider,
            sources: vec![(id, path)],
            fps,
            playhead: 0,
            playing: false,
            play_origin: 0,
            play_start: Instant::now(),
            audio: AudioPreview::new(),
            audio_dirty: true,
            texture: None,
            rendered: None,
            dirty: true,
            active_track: v,
            selected: None,
            drag: None,
            export: None,
            status: None,
        };
        editor.ensure_preview(ctx);
        Ok(editor)
    }

    /// Last frame index (one before the timeline end), or 0 for an empty edit.
    fn last_frame(&self) -> u64 {
        self.timeline.duration_frames().saturating_sub(1)
    }

    fn fps_f64(&self) -> f64 {
        self.fps.as_f64().max(1.0)
    }

    fn toggle_play(&mut self) {
        if self.playing {
            self.stop_playback();
        } else if self.timeline.duration_frames() > 0 {
            // Restart from the top if parked at the end.
            if self.playhead >= self.last_frame() {
                self.playhead = 0;
            }
            self.ensure_mix();
            self.play_origin = self.playhead;
            self.play_start = Instant::now();
            self.playing = true;
            if let Some(audio) = &self.audio {
                audio.play_from(self.timeline.frame_to_sample(self.playhead));
            }
        }
    }

    /// Stop playback and silence the audio (without moving the playhead).
    fn stop_playback(&mut self) {
        self.playing = false;
        if let Some(audio) = &self.audio {
            audio.stop();
        }
    }

    fn seek(&mut self, frame: u64) {
        self.stop_playback();
        self.playhead = frame.min(self.last_frame());
    }

    fn step(&mut self, delta: i64) {
        self.stop_playback();
        let t = (self.playhead as i64 + delta).clamp(0, self.last_frame() as i64);
        self.playhead = t as u64;
    }

    fn split_at_playhead(&mut self) {
        if self.timeline.split(self.active_track, self.playhead) {
            self.selected = None;
            self.mark_edited();
        }
    }

    fn delete_selected(&mut self) {
        if let Some((track, clip)) = self.selected.take() {
            self.timeline.ripple_delete(track, clip);
            self.mark_edited();
        }
    }

    /// Flag the preview + audio mix stale after an edit.
    fn mark_edited(&mut self) {
        self.dirty = true;
        self.audio_dirty = true;
    }

    /// Advance the playhead from the wall clock while playing.
    fn advance_playback(&mut self, ctx: &egui::Context) {
        if !self.playing {
            return;
        }
        let dur = self.timeline.duration_frames();
        if dur == 0 {
            self.stop_playback();
            return;
        }
        let elapsed = self.play_start.elapsed().as_secs_f64();
        let pos = self.play_origin + (elapsed * self.fps_f64()) as u64;
        if pos >= dur {
            self.playhead = dur - 1;
            self.stop_playback();
        } else {
            self.playhead = pos;
        }
        ctx.request_repaint();
    }

    /// Recompute the mixed audio for the whole edit and hand it to the output, if
    /// the mix is stale. Chunked + capped so memory stays bounded.
    fn ensure_mix(&mut self) {
        if !self.audio_dirty {
            return;
        }
        self.audio_dirty = false;
        let Some(audio) = &self.audio else {
            return;
        };
        if !self.timeline.has_audio() {
            audio.set_mix(Arc::new(Vec::new()));
            return;
        }
        let total = self
            .timeline
            .frame_to_sample(self.timeline.duration_frames());
        let n = total.min(MAX_PREVIEW_SAMPLES) as usize;
        let ch = self.timeline.channels.max(1) as usize;
        let mut mix = Vec::with_capacity(n * ch);
        let mut s = 0usize;
        while s < n {
            let len = (n - s).min(48_000);
            mix.extend_from_slice(&compose_audio(
                &self.timeline,
                &mut self.provider,
                s as u64,
                len,
            ));
            s += len;
        }
        audio.set_mix(Arc::new(mix));
    }

    /// Recompose the preview texture for the current playhead if it changed.
    fn ensure_preview(&mut self, ctx: &egui::Context) {
        let t = self.playhead.min(self.last_frame());
        if self.texture.is_some() && self.rendered == Some(t) && !self.dirty {
            return;
        }
        let frame = compose_frame(&self.timeline, &mut self.provider, t);
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [frame.width as usize, frame.height as usize],
            &frame.pixels,
        );
        match &mut self.texture {
            Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
            None => {
                self.texture =
                    Some(ctx.load_texture("timeline_preview", image, egui::TextureOptions::LINEAR));
            }
        }
        self.rendered = Some(t);
        self.dirty = false;
    }

    /// Draw the editor and report whether the user closed it.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> TimelineOutcome {
        let ctx = ui.ctx().clone();
        let mut outcome = TimelineOutcome::Active;

        self.advance_playback(&ctx);
        self.poll_export(&ctx);
        self.ensure_preview(&ctx);

        egui::Panel::bottom("timeline_controls").show_inside(ui, |ui| {
            ui.add_space(4.0);
            self.transport_bar(ui, &mut outcome);
            ui.add_space(4.0);
            self.timeline_strip(ui);
            self.inspector_row(ui);
            self.status_row(ui);
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let avail = ui.available_size();
            match &self.texture {
                Some(tex) => {
                    let fit = fit_within(tex.size_vec2(), avail);
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
            outcome = TimelineOutcome::Close;
        }
        outcome
    }

    /// The transport + edit controls row.
    fn transport_bar(&mut self, ui: &mut egui::Ui, outcome: &mut TimelineOutcome) {
        let exporting = self.export.is_some();
        let has_selection = self.selected.is_some();
        let mut export_request: Option<(ExportFormat, PathBuf)> = None;

        ui.horizontal(|ui| {
            if ui.button("⏮").on_hover_text("Go to start").clicked() {
                self.seek(0);
            }
            if ui
                .button("◀")
                .on_hover_text("Step back one frame")
                .clicked()
            {
                self.step(-1);
            }
            let play = if self.playing { "⏸" } else { "▶" };
            if ui
                .button(play)
                .on_hover_text("Play / pause (Space)")
                .clicked()
            {
                self.toggle_play();
            }
            if ui
                .button("▶|")
                .on_hover_text("Step forward one frame")
                .clicked()
            {
                self.step(1);
            }
            if ui.button("⏭").on_hover_text("Go to end").clicked() {
                self.seek(self.last_frame());
            }

            ui.separator();
            if ui
                .button("Split")
                .on_hover_text("Split the clip under the playhead (active track)")
                .clicked()
            {
                self.split_at_playhead();
            }
            if ui
                .add_enabled(has_selection, egui::Button::new("Delete"))
                .on_hover_text("Ripple-delete the selected clip (closes the gap)")
                .clicked()
            {
                self.delete_selected();
            }

            ui.separator();
            let tc = Timecode::from_frame(self.playhead, self.fps);
            let end = Timecode::from_frame(self.last_frame(), self.fps);
            ui.monospace(format!("{}  ·  f{}", tc.smpte(), self.playhead));
            ui.label(egui::RichText::new(format!("/ {}", end.smpte())).weak());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    *outcome = TimelineOutcome::Close;
                }
                ui.add_enabled_ui(!exporting, |ui| {
                    ui.menu_button("Export ▾", |ui| {
                        for fmt in ExportFormat::ALL {
                            if ui.button(fmt.label()).clicked() {
                                if let Some(dst) = self.save_dialog(fmt) {
                                    export_request = Some((fmt, dst));
                                }
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text(
                        "Renders the timeline, then encodes (WebM/MP4 via ffmpeg; GIF built-in)",
                    );
                });
                ui.label(
                    egui::RichText::new(format!(
                        "{} × {} · {:.0} fps",
                        self.timeline.width,
                        self.timeline.height,
                        self.fps_f64()
                    ))
                    .weak()
                    .small(),
                );
            });
        });

        if let Some((format, dst)) = export_request {
            self.start_export(ui.ctx(), format, dst);
        }
    }

    /// The multi-track clip strip: lanes, clips, playhead, and drag interactions
    /// (scrub on empty space, move a clip body, trim a clip edge).
    fn timeline_strip(&mut self, ui: &mut egui::Ui) {
        let n = self.timeline.tracks.len().max(1);
        let dur = self.timeline.duration_frames().max(1);
        let desired = egui::vec2(ui.available_width(), n as f32 * (ROW_H + GAP) + GAP);
        let (rect, resp) = ui.allocate_exact_size(desired, egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        let left = rect.left() + PAD;
        let ppf = (rect.width() - 2.0 * PAD).max(1.0) / dur as f32;

        painter.rect_filled(rect, 0.0, egui::Color32::from_gray(28));

        // Draw highest track index at the top (NLE convention).
        for (row, ti) in (0..n).rev().enumerate() {
            let y = row_top_y(rect, row);
            let lane = egui::Rect::from_min_size(
                egui::pos2(left, y),
                egui::vec2((rect.width() - 2.0 * PAD).max(1.0), ROW_H),
            );
            painter.rect_filled(lane, 2.0, egui::Color32::from_gray(42));
            if ti == self.active_track {
                painter.rect_stroke(
                    lane,
                    2.0,
                    egui::Stroke::new(1.0, egui::Color32::from_gray(120)),
                    egui::StrokeKind::Inside,
                );
            }
            let Some(track) = self.timeline.tracks.get(ti) else {
                continue;
            };
            let base = match track.kind {
                TrackKind::Video => egui::Color32::from_rgb(58, 104, 168),
                TrackKind::Audio => egui::Color32::from_rgb(66, 150, 96),
            };
            for (ci, clip) in track.clips.iter().enumerate() {
                let x0 = x_at_frame(clip.start, left, ppf);
                let x1 = x_at_frame(clip.end(), left, ppf).max(x0 + 2.0);
                let crect = egui::Rect::from_min_max(
                    egui::pos2(x0, y + 1.0),
                    egui::pos2(x1, y + ROW_H - 1.0),
                );
                let fill = if clip.enabled {
                    base
                } else {
                    egui::Color32::from_gray(70)
                };
                painter.rect_filled(crect, 3.0, fill);
                if self.selected == Some((ti, ci)) {
                    painter.rect_stroke(
                        crect,
                        3.0,
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 210, 80)),
                        egui::StrokeKind::Middle,
                    );
                }
                if x1 - x0 > 26.0 {
                    painter.text(
                        crect.left_center() + egui::vec2(5.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        &track.name,
                        egui::FontId::proportional(11.0),
                        egui::Color32::from_white_alpha(220),
                    );
                }
            }
        }

        // Playhead.
        let px = x_at_frame(self.playhead.min(dur), left, ppf);
        painter.line_segment(
            [egui::pos2(px, rect.top()), egui::pos2(px, rect.bottom())],
            egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 80, 80)),
        );

        self.handle_strip_input(ui, &resp, rect, left, ppf, n, dur);
    }

    /// Drive selection, scrubbing, moves, and trims from the strip's response.
    #[allow(clippy::too_many_arguments)]
    fn handle_strip_input(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: egui::Rect,
        left: f32,
        ppf: f32,
        n: usize,
        dur: u64,
    ) {
        // Hover cursor feedback over a clip edge / body.
        if self.drag.is_none() && resp.hovered() {
            if let Some(p) = resp.hover_pos() {
                match self.op_at(p, rect, left, ppf, n) {
                    DragOp::TrimHead { .. } | DragOp::TrimTail { .. } => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    DragOp::Move { .. } => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
                    DragOp::Scrub => {}
                }
            }
        }

        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.drag = Some(self.begin_drag(p, rect, left, ppf, n));
            }
        }
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.apply_drag(p, left, ppf, dur);
            }
        }
        if resp.drag_stopped() {
            if matches!(
                self.drag,
                Some(DragOp::Move { .. } | DragOp::TrimHead { .. } | DragOp::TrimTail { .. })
            ) {
                self.mark_edited();
            }
            self.drag = None;
        }

        // A plain click (no drag) selects a clip or seeks.
        if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                match self.clip_at_pos(p, rect, left, ppf, n) {
                    Some((ti, ci)) => {
                        self.selected = Some((ti, ci));
                        self.active_track = ti;
                    }
                    None => self.seek(frame_at_x(p.x, left, ppf)),
                }
            }
        }
    }

    /// Classify what a drag starting at `p` would do (no mutation).
    fn op_at(&self, p: egui::Pos2, rect: egui::Rect, left: f32, ppf: f32, n: usize) -> DragOp {
        let Some((ti, ci)) = self.clip_at_pos(p, rect, left, ppf, n) else {
            return DragOp::Scrub;
        };
        let clip = &self.timeline.tracks[ti].clips[ci];
        let dl = (p.x - x_at_frame(clip.start, left, ppf)).abs();
        let dr = (p.x - x_at_frame(clip.end(), left, ppf)).abs();
        if dl <= EDGE_GRAB && dl <= dr {
            DragOp::TrimHead {
                track: ti,
                clip: ci,
            }
        } else if dr <= EDGE_GRAB {
            DragOp::TrimTail {
                track: ti,
                clip: ci,
            }
        } else {
            DragOp::Move {
                track: ti,
                clip: ci,
                grab_offset: 0,
            }
        }
    }

    /// Begin a drag: select the grabbed clip and record the grab offset for moves.
    fn begin_drag(
        &mut self,
        p: egui::Pos2,
        rect: egui::Rect,
        left: f32,
        ppf: f32,
        n: usize,
    ) -> DragOp {
        let op = self.op_at(p, rect, left, ppf, n);
        match op {
            DragOp::Scrub => DragOp::Scrub,
            DragOp::Move { track, clip, .. } => {
                self.selected = Some((track, clip));
                self.active_track = track;
                let start = self.timeline.tracks[track].clips[clip].start as i64;
                DragOp::Move {
                    track,
                    clip,
                    grab_offset: start - frame_at_x(p.x, left, ppf) as i64,
                }
            }
            DragOp::TrimHead { track, clip } | DragOp::TrimTail { track, clip } => {
                self.selected = Some((track, clip));
                self.active_track = track;
                op
            }
        }
    }

    /// Apply the active drag at pointer position `p`.
    fn apply_drag(&mut self, p: egui::Pos2, left: f32, ppf: f32, dur: u64) {
        let pf = frame_at_x(p.x, left, ppf);
        match self.drag {
            Some(DragOp::Scrub) => {
                self.stop_playback();
                self.playhead = pf.min(dur.saturating_sub(1));
            }
            Some(DragOp::Move {
                track,
                clip,
                grab_offset,
            }) => {
                let target = (pf as i64 + grab_offset).max(0) as u64;
                self.timeline.move_clip(track, clip, target);
                self.dirty = true;
            }
            Some(DragOp::TrimHead { track, clip }) => {
                self.timeline.trim_head(track, clip, pf);
                self.dirty = true;
            }
            Some(DragOp::TrimTail { track, clip }) => {
                self.timeline.trim_tail(track, clip, pf);
                self.dirty = true;
            }
            None => {}
        }
    }

    /// Hit-test a pointer position against the clip rectangles in the strip.
    fn clip_at_pos(
        &self,
        p: egui::Pos2,
        rect: egui::Rect,
        left: f32,
        ppf: f32,
        n: usize,
    ) -> Option<(usize, usize)> {
        for (row, ti) in (0..n).rev().enumerate() {
            let y = row_top_y(rect, row);
            if p.y < y || p.y > y + ROW_H {
                continue;
            }
            let track = self.timeline.tracks.get(ti)?;
            for (ci, clip) in track.clips.iter().enumerate() {
                if p.x >= x_at_frame(clip.start, left, ppf)
                    && p.x <= x_at_frame(clip.end(), left, ppf)
                {
                    return Some((ti, ci));
                }
            }
        }
        None
    }

    /// The selected-clip inspector: enable, opacity, gain, mute, and fades.
    fn inspector_row(&mut self, ui: &mut egui::Ui) {
        let Some((ti, ci)) = self.selected else {
            return;
        };
        let Some(clip) = self
            .timeline
            .tracks
            .get_mut(ti)
            .and_then(|t| t.clips.get_mut(ci))
        else {
            self.selected = None;
            return;
        };
        let max_fade = clip.duration();
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Clip:");
            changed |= ui.checkbox(&mut clip.enabled, "On").changed();
            ui.separator();
            ui.label("Opacity");
            changed |= ui
                .add(egui::Slider::new(&mut clip.opacity, 0.0..=1.0))
                .changed();
            ui.separator();
            ui.label("Gain");
            changed |= ui
                .add(egui::Slider::new(&mut clip.gain, 0.0..=2.0))
                .changed();
            changed |= ui.checkbox(&mut clip.audio_muted, "Mute").changed();
            ui.separator();
            ui.label("Fade in/out");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut clip.fade_in)
                        .range(0..=max_fade)
                        .suffix("f"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut clip.fade_out)
                        .range(0..=max_fade)
                        .suffix("f"),
                )
                .changed();
        });
        if changed {
            self.mark_edited();
        }
    }

    /// The export-progress / result line.
    fn status_row(&mut self, ui: &mut egui::Ui) {
        match &self.export {
            Some(Exporting::Rendering { job, .. }) => {
                ui.horizontal(|ui| {
                    ui.label(format!("Rendering timeline … {:.0}%", job.progress * 100.0));
                    ui.spinner();
                });
            }
            Some(Exporting::Encoding { job, .. }) => {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Encoding {} … {:.0}%",
                        job.format.label(),
                        job.progress * 100.0
                    ));
                    ui.spinner();
                });
            }
            None => {
                if let Some(msg) = &self.status {
                    ui.label(egui::RichText::new(msg).weak());
                }
            }
        }
    }

    /// Start the render→encode pipeline for `format` to `dst`.
    fn start_export(&mut self, ctx: &egui::Context, format: ExportFormat, dst: PathBuf) {
        if self.timeline.duration_frames() == 0 {
            self.status = Some("Nothing to export — the timeline is empty.".to_owned());
            return;
        }
        let temp = temp_fvid_path();
        let job = RenderJob::start(
            ctx,
            self.timeline.clone(),
            self.sources.clone(),
            temp.clone(),
        );
        self.status = None;
        self.export = Some(Exporting::Rendering {
            job,
            format,
            dst,
            temp,
        });
    }

    /// Drive the export pipeline forward each frame.
    fn poll_export(&mut self, ctx: &egui::Context) {
        let Some(state) = self.export.take() else {
            return;
        };
        self.export = match state {
            Exporting::Rendering {
                mut job,
                format,
                dst,
                temp,
            } => {
                job.poll();
                match job.done.take() {
                    None => {
                        ctx.request_repaint();
                        Some(Exporting::Rendering {
                            job,
                            format,
                            dst,
                            temp,
                        })
                    }
                    Some(Ok(())) => {
                        let job = ExportJob::start(ctx, temp.clone(), dst, format);
                        ctx.request_repaint();
                        Some(Exporting::Encoding { job, temp })
                    }
                    Some(Err(e)) => {
                        let _ = std::fs::remove_file(&temp);
                        self.status = Some(format!("Export failed while rendering: {e}"));
                        None
                    }
                }
            }
            Exporting::Encoding { mut job, temp } => {
                job.poll();
                match &job.done {
                    None => {
                        ctx.request_repaint();
                        Some(Exporting::Encoding { job, temp })
                    }
                    Some(result) => {
                        let _ = std::fs::remove_file(&temp);
                        self.status = Some(match result {
                            Ok(path) => format!("Exported to {}", path.display()),
                            Err(e) => format!("Export failed: {e}"),
                        });
                        None
                    }
                }
            }
        };
    }

    /// Native save dialog defaulting to the first source's folder.
    fn save_dialog(&self, format: ExportFormat) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new()
            .set_file_name(format!("timeline.{}", format.ext()))
            .add_filter(format.label(), &[format.ext()]);
        if let Some(parent) = self
            .sources
            .first()
            .and_then(|(_, p)| p.parent())
            .filter(|p| p.is_dir())
        {
            dialog = dialog.set_directory(parent);
        }
        dialog.save_file()
    }
}

/// X position (points) of timeline frame `f` in the strip.
fn x_at_frame(f: u64, left: f32, ppf: f32) -> f32 {
    left + f as f32 * ppf
}

/// Timeline frame nearest strip X position `x`.
fn frame_at_x(x: f32, left: f32, ppf: f32) -> u64 {
    (((x - left) / ppf).round()).max(0.0) as u64
}

/// Top Y (points) of strip lane `row`.
fn row_top_y(rect: egui::Rect, row: usize) -> f32 {
    rect.top() + GAP + row as f32 * (ROW_H + GAP)
}

/// A background job that composites the whole timeline to a temp `.fvid`.
struct RenderJob {
    rx: Receiver<RenderMsg>,
    progress: f32,
    done: Option<Result<(), String>>,
}

enum RenderMsg {
    Progress(f32),
    Done(Result<(), String>),
}

impl RenderJob {
    fn start(
        ctx: &egui::Context,
        timeline: Timeline,
        sources: Vec<(MediaId, PathBuf)>,
        dst: PathBuf,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("freally-timeline-render".to_owned())
            .spawn(move || {
                let result = render_to_fvid(&timeline, sources, &dst, &tx, &ctx);
                let _ = tx.send(RenderMsg::Done(result));
                ctx.request_repaint();
            });
        if spawned.is_err() {
            let (tx2, rx2) = mpsc::channel();
            let _ = tx2.send(RenderMsg::Done(Err(
                "Couldn't start the render thread".to_owned()
            )));
            return Self {
                rx: rx2,
                progress: 0.0,
                done: None,
            };
        }
        Self {
            rx,
            progress: 0.0,
            done: None,
        }
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                RenderMsg::Progress(p) => self.progress = p,
                RenderMsg::Done(r) => self.done = Some(r),
            }
        }
    }
}

/// Composite every timeline frame (and frame-aligned audio) to a `.fvid` at `dst`.
fn render_to_fvid(
    timeline: &Timeline,
    sources: Vec<(MediaId, PathBuf)>,
    dst: &Path,
    tx: &Sender<RenderMsg>,
    ctx: &egui::Context,
) -> Result<(), String> {
    let dur = timeline.duration_frames();
    if dur == 0 {
        return Err("the timeline is empty".to_owned());
    }
    // A fresh provider for this thread (readers can't be shared with the UI).
    let mut provider = AppProvider::new();
    for (id, path) in sources {
        provider.insert(id, FvidReader::open(&path)?);
    }

    let (w, h, fps) = (timeline.width, timeline.height, timeline.fps);
    let has_audio = timeline.has_audio();
    let mut enc = if has_audio {
        StreamEncoder::create_with_audio(dst, w, h, fps, timeline.sample_rate, timeline.channels)
    } else {
        StreamEncoder::create(dst, w, h, fps)
    }
    .map_err(|e| e.to_string())?;

    for t in 0..dur {
        let frame = compose_frame(timeline, &mut provider, t);
        enc.push_frame(&frame).map_err(|e| e.to_string())?;
        if has_audio {
            let s0 = timeline.frame_to_sample(t);
            let s1 = timeline.frame_to_sample(t + 1);
            let block = compose_audio(timeline, &mut provider, s0, (s1 - s0) as usize);
            enc.push_audio(&block).map_err(|e| e.to_string())?;
        }
        let _ = tx.send(RenderMsg::Progress((t + 1) as f32 / dur as f32));
        ctx.request_repaint();
    }
    enc.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// A unique temp path for an intermediate render.
fn temp_fvid_path() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "freally_timeline_{}_{seq}.fvid",
        std::process::id()
    ))
}

/// Scale `size` to fit `avail`, preserving aspect ratio.
fn fit_within(size: egui::Vec2, avail: egui::Vec2) -> egui::Vec2 {
    let (w, h) = (size.x.max(1.0), size.y.max(1.0));
    let scale = (avail.x / w).min(avail.y / h).max(0.0);
    egui::vec2(w * scale, h * scale)
}
