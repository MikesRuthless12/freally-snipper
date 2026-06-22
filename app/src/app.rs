//! The application: the Win11-style home window plus the Phase 1 capture flow
//! (hide the window → grab the desktop → selection overlay → save the result).
//!
//! The capture state machine runs in [`eframe::App::logic`], which eframe calls
//! every frame *even while the home window is hidden* — so we can hide the home
//! chrome, grab a clean shot, and only then bring the window back as a
//! full-desktop selection overlay. Rendering happens in [`eframe::App::ui`]: the
//! home window (toolbar + Capture / Settings / About views) when idle, or the
//! overlay (drawn into this same window) while a capture is in progress. We
//! reuse the one OS window rather than a child viewport because immediate child
//! viewports cannot render while their parent is hidden.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use freally_capture::image::RgbaImage;
use freally_capture::{Composite, Rect as VRect, WindowInfo};

use crate::audio::AudioConfig;
use crate::delivery::Delivery;
use crate::gallery::Gallery;
use crate::hotkey::Hotkeys;
use crate::overlay::{apply_selection, OverlayOutcome, OverlaySession, RecordGeom, Selection};
use crate::player::{Player, PlayerOutcome};
use crate::print_screen::{self, KeyOutcome};
use crate::recorder::{RecordConfig, RecordTarget, Recorder};
use crate::settings::{
    language_native, ImageFormat, Settings, SnippetMode, Theme, TimerDelay, UI_LANGUAGES,
};
use crate::timeline::{TimelineEditor, TimelineOutcome};
use crate::tray::{Tray, TrayCommand};
use freally_editor::{EditorOutcome, EditorSession};

/// Delay between hiding the home window and grabbing the screen, so the window is
/// actually gone from the shot before the snapshot is taken. The user's Timer ▾
/// delay is added on top of this.
const HIDE_DELAY: Duration = Duration::from_millis(150);

/// Default home-window size, restored after a capture.
const HOME_SIZE: egui::Vec2 = egui::vec2(900.0, 600.0);

/// Side length of a recent-capture thumbnail tile, in points.
const TILE: f32 = 96.0;

/// License + attribution text, embedded so the About panel always has them with
/// no runtime file lookup (works in the installed app too).
const LICENSE_TEXT: &str = include_str!("../../LICENSE");
const THIRD_PARTY_TEXT: &str = include_str!("../../THIRD-PARTY-NOTICES.md");
const EULA_TEXT: &str = include_str!("../../EULA.md");
const PRIVACY_TEXT: &str = include_str!("../../PRIVACY.md");

/// Which home-window view is showing (toolbar is always visible above it).
#[derive(Clone, Copy, PartialEq, Eq)]
enum HomeView {
    Capture,
    Settings,
    About,
}

pub struct FreallySnipperApp {
    settings: Settings,
    capture: CaptureState,
    /// Global capture hotkey, or `None` if the OS would not grant it.
    hotkeys: Option<Hotkeys>,
    /// Background worker that copies captures to the clipboard and saves them,
    /// so committing a capture never blocks the UI.
    delivery: Delivery,
    /// Off-thread thumbnail decoder + cache for the recent-captures strip.
    gallery: Gallery,
    /// Home-window position remembered across a capture, to restore afterwards.
    home_pos: Option<egui::Pos2>,
    /// Last-action message shown on the home window (saved path / cancelled / error).
    status: Option<String>,
    /// Which home view is showing.
    view: HomeView,
    /// macOS/Linux guidance for the Print-Screen option: (message, optional
    /// settings deep link to open on request).
    print_screen_guidance: Option<(String, Option<String>)>,
    /// Settings changed but not yet written. Persistence is deferred while the
    /// pointer is held (e.g. dragging the colour picker) and flushed on release,
    /// so a drag doesn't rewrite settings.json every frame.
    needs_persist: bool,
    /// System-tray icon (Windows/macOS) that keeps the app resident — so the
    /// global hotkey / Print Screen still work — while the window is closed.
    /// `None` if unavailable (e.g. Linux, or creation failed).
    tray: Option<Tray>,
    /// Set when the user chose Quit from the tray, so the next close actually exits
    /// instead of hiding to the tray.
    quitting: bool,
    /// True while the window is hidden in the tray, so a capture taken via the
    /// hotkey returns to the tray instead of popping the window back up.
    hidden_to_tray: bool,
}

enum CaptureState {
    Idle,
    /// Home window hidden; waiting `HIDE_DELAY` (hide settle) before grabbing the
    /// frozen snapshot used for the selection overlay / an immediate capture.
    Arming {
        mode: SnippetMode,
        since: Instant,
        /// Whether the Video capture type is armed (a committed selection starts a
        /// recording instead of a photo).
        record: bool,
    },
    /// Selection overlay is live with a frozen snapshot.
    Active(Box<OverlaySession>),
    /// Post-capture image editor (Toolbar 2, P4.1): the capture is shown on a
    /// zoom/pan canvas in a decorated editor window with Save / Copy / Discard.
    /// Reached only when Markup is armed (else the capture saves directly).
    Editing(Box<EditorSession>),
    /// A visible countdown badge is showing before the *live* snapshot (Timer ▾).
    /// `selection` is `None` for full screen (re-grab the whole desktop) or the
    /// chosen region for Rectangle / Window / Freeform — grabbed live after the
    /// countdown so the shot reflects whatever the user set up during the delay.
    /// `markup` carries the editor-hand-off choice across the countdown.
    Counting {
        since: Instant,
        total: Duration,
        selection: Option<Selection>,
        markup: bool,
    },
    /// Badge hidden; waiting `HIDE_DELAY` before grabbing the *live* desktop for a
    /// timed capture (then crop/mask to `selection`, or keep the whole desktop).
    GrabbingLive {
        selection: Option<Selection>,
        since: Instant,
        markup: bool,
    },
    /// A screen recording is in progress (P5.1): the window is a small always-on-top
    /// control bar (REC · time · Pause · Stop) placed outside the recorded area.
    Recording(Box<Recording>),
    /// Playing back a saved `.fvid` recording (P5.1) in a decorated player window.
    Playing(Box<Player>),
    /// Editing a recording on the Phase 6 timeline (P6.1) in a decorated window.
    Timeline(Box<TimelineEditor>),
}

/// Live state for an in-progress screen recording (P5.1).
struct Recording {
    /// Handle to the background recorder thread.
    recorder: Recorder,
    /// The recorded area, in virtual-desktop pixels — places the control bar
    /// outside the shot and labels the recording.
    rect: VRect,
    /// Whether this attaches to and follows a window (vs a fixed region).
    follows_window: bool,
    /// Latest elapsed time from the worker (excludes paused spans).
    elapsed: Duration,
    /// Whether the recording is currently paused.
    paused: bool,
    /// Set once Stop is requested, so the bar shows "Saving…" until the worker
    /// reports the finished file.
    stopping: bool,
}

impl FreallySnipperApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        mut settings: Settings,
        icon: Option<egui::IconData>,
        minimized: bool,
    ) -> Self {
        apply_theme(&cc.egui_ctx, settings.theme);

        let mut hotkeys = Hotkeys::new(&cc.egui_ctx);
        let mut status = None;
        if let Some(h) = &mut hotkeys {
            if !h.set_hotkey(&settings.hotkey) {
                status = Some(format!(
                    "Hotkey \"{}\" could not be registered — use + New instead.",
                    settings.hotkey
                ));
            }
            // Re-register Print Screen if the user previously opted in (P1.5).
            // Surface a failure: VK_SNAPSHOT can be refused until Windows releases
            // the Snipping Tool's claim on the key (often needs a sign-out).
            if settings.open_with_print_screen && !h.set_print_screen(true) {
                status = Some(
                    "Print Screen couldn't be registered — another app may hold it, or Windows \
                     needs a sign-out to release it. The capture hotkey and + New still work."
                        .to_owned(),
                );
            }
        }

        // Drop recents whose files were deleted since last run.
        let before = settings.recent_captures.len();
        settings.prune_recent();
        let pruned = settings.recent_captures.len() != before;

        // System tray (Windows/macOS) reuses the app icon, but pre-scaled to a
        // small, crisp square — Windows renders the tray slot at ~16–32px, and
        // letting it crush the full-res icon there looks distorted/blurry.
        // When launched `--minimized` (start-at-login), show the tray so the
        // hidden window can be reopened, even if minimize-to-tray is off.
        let tray = icon.and_then(|icon| {
            let (rgba, w, h) = tray_icon_rgba(&icon, 64);
            Tray::new(
                &cc.egui_ctx,
                rgba,
                w,
                h,
                settings.minimize_to_tray || minimized,
            )
        });

        let app = Self {
            settings,
            capture: CaptureState::Idle,
            hotkeys,
            delivery: Delivery::new(&cc.egui_ctx),
            gallery: Gallery::new(&cc.egui_ctx),
            home_pos: None,
            status,
            view: HomeView::Capture,
            print_screen_guidance: None,
            needs_persist: false,
            tray,
            quitting: false,
            // Launched minimized → the window starts hidden (NativeOptions), so a
            // hotkey capture returns to the tray instead of popping it open.
            hidden_to_tray: minimized,
        };
        if pruned {
            app.persist();
        }
        app
    }

    fn persist(&self) {
        if let Err(err) = self.settings.save() {
            eprintln!("Freally Snipper: could not save settings: {err}");
        }
    }

    /// Start a capture: remember the home position, hide the chrome, then arm
    /// (hide settle) before the frozen snapshot. The Timer ▾ countdown — when set
    /// — runs *after* the selection, so the user picks the region first, then the
    /// screen is grabbed live after the delay.
    fn begin_capture(&mut self, ctx: &egui::Context, mode: SnippetMode, record: bool) {
        if !matches!(self.capture, CaptureState::Idle) {
            return;
        }
        self.status = None;
        // Keep the last known home position: when triggered from the tray the
        // window is hidden, so `outer_rect` is None — don't overwrite a good value.
        self.home_pos = ctx
            .input(|i| i.viewport().outer_rect)
            .map(|r| r.min)
            .or(self.home_pos);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        self.capture = CaptureState::Arming {
            mode,
            since: Instant::now(),
            record,
        };
        ctx.request_repaint();
    }

    /// Begin the Timer ▾ countdown (morph to the badge), to be followed by the
    /// live snapshot. `selection` is `None` for full screen, else the region;
    /// `markup` carries whether to open the editor on the resulting capture.
    fn start_countdown(&mut self, ctx: &egui::Context, selection: Option<Selection>, markup: bool) {
        morph_to_countdown_badge(ctx, screen_center(ctx));
        self.capture = CaptureState::Counting {
            since: Instant::now(),
            total: self.settings.timer_delay.duration(),
            selection,
            markup,
        };
        ctx.request_repaint();
    }

    /// Advance the capture state machine. Called from `App::logic` every frame
    /// (including while the home window is hidden).
    fn tick(&mut self, ctx: &egui::Context) {
        // Reflect finished background deliveries: record saved files in the
        // gallery's recents and show the latest status line.
        let results = self.delivery.poll();
        if !results.is_empty() {
            let mut recents_changed = false;
            for result in &results {
                if let Some(path) = &result.saved_path {
                    self.settings.push_recent(path.clone());
                    recents_changed = true;
                }
            }
            if let Some(last) = results.last() {
                self.status = Some(last.message.clone());
            }
            if recents_changed {
                self.persist();
            }
        }

        // Upload any thumbnails that finished decoding off-thread.
        self.gallery.pump(ctx);

        // Reflect recording progress + finalize a finished recording (P5.1).
        self.poll_recorder(ctx);

        // A global-hotkey press opens the overlay (only honored while idle).
        let hotkey_fired = self.hotkeys.as_ref().is_some_and(Hotkeys::take_fired);
        if hotkey_fired {
            self.begin_capture(ctx, self.settings.default_snippet_mode, false);
        }

        // Quit from the tray during a recording: stop + save, then quit once the
        // recording is finalized (see `finish_recording`) — otherwise the Quit would
        // queue behind the idle-gated handler below and appear to do nothing.
        if let CaptureState::Recording(rec) = &mut self.capture {
            if let Some(TrayCommand::Quit) = self.tray.as_ref().and_then(Tray::poll) {
                rec.recorder.stop();
                rec.stopping = true;
                self.quitting = true;
            }
        }

        // System tray (Windows/macOS): act on Open / Quit commands — but only while
        // idle, so a tray click during a capture can't pop the window into the shot
        // or cancel a quit mid-snip (commands queue and run once the capture ends).
        if matches!(self.capture, CaptureState::Idle) {
            match self.tray.as_ref().and_then(Tray::poll) {
                Some(TrayCommand::Open) => {
                    self.hidden_to_tray = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Some(TrayCommand::Quit) => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                None => {}
            }
        }
        // With minimize-to-tray on, the window's close button hides it (keeping the
        // hotkey alive) rather than quitting — unless Quit was chosen from the tray.
        if matches!(self.capture, CaptureState::Idle)
            && self.settings.minimize_to_tray
            && self.tray.is_some()
            && !self.quitting
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.hidden_to_tray = true;
        }

        match &self.capture {
            CaptureState::Idle => {}
            CaptureState::Arming {
                mode,
                since,
                record,
            } => {
                let mode = *mode;
                let record = *record;
                if since.elapsed() < HIDE_DELAY {
                    // Drive the wait with an unconditional repaint: the home window
                    // is hidden here, and a hidden eframe viewport does NOT reliably
                    // honor request_repaint_after, so anything else makes the overlay
                    // take "forever" to appear.
                    ctx.request_repaint();
                    return;
                }
                self.snapshot_after_arming(ctx, mode, record);
            }
            CaptureState::Active(_) => {
                // Rendering + input happen in `ui`; just keep frames flowing.
                ctx.request_repaint();
            }
            CaptureState::Editing(_) => {
                // The editor is a static surface; egui repaints on input. Nothing
                // to drive here.
            }
            CaptureState::Counting { since, total, .. } => {
                if since.elapsed() < *total {
                    // The badge is visible; wake a few times a second (enough for a
                    // 1 Hz digit) instead of busy-repainting at the refresh rate.
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    return;
                }
                // Countdown done: hide the badge and settle before the live grab,
                // so the badge never lands in the shot.
                let (selection, markup) =
                    match std::mem::replace(&mut self.capture, CaptureState::Idle) {
                        CaptureState::Counting {
                            selection, markup, ..
                        } => (selection, markup),
                        _ => (None, false),
                    };
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.capture = CaptureState::GrabbingLive {
                    selection,
                    since: Instant::now(),
                    markup,
                };
                ctx.request_repaint();
            }
            CaptureState::GrabbingLive { since, .. } => {
                if since.elapsed() < HIDE_DELAY {
                    ctx.request_repaint();
                    return;
                }
                self.grab_live(ctx);
            }
            CaptureState::Recording(_) => {
                // The worker repaints on each progress update; keep a slow heartbeat
                // so the elapsed clock stays live even between frames.
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
            CaptureState::Playing(_) => {
                // The player drives its own repaints while playing.
            }
            CaptureState::Timeline(_) => {
                // The timeline editor drives its own repaints while playing/exporting.
            }
        }
    }

    /// After the hide settle: grab the frozen desktop, then either capture
    /// immediately (full screen, Timer Off), open the selection overlay, or — for
    /// full screen *with* a Timer — start the countdown to a live grab.
    fn snapshot_after_arming(&mut self, ctx: &egui::Context, mode: SnippetMode, record: bool) {
        let timed = self.settings.timer_delay != TimerDelay::None;
        // Whether to open the editor after the capture — the persisted setting is
        // the starting point; the overlay's Markup button can flip it per-capture.
        let markup = self.settings.show_capture_editor;
        // Full screen + Timer (photo only) needs no frozen snapshot — count down,
        // then grab the live desktop. Recording always goes through the overlay so
        // the user commits a target (region / window / full screen).
        if !record && mode == SnippetMode::FullScreen && timed {
            self.start_countdown(ctx, None, markup);
            return;
        }
        match capture_desktop() {
            Ok((composite, windows)) => {
                if !record && mode == SnippetMode::FullScreen {
                    self.deliver_or_edit(ctx, composite.into_image(), markup);
                } else {
                    let bounds = composite.bounds;
                    let session = OverlaySession::new(
                        ctx,
                        composite,
                        mode,
                        windows,
                        timed && !record,
                        self.settings.active_color,
                        markup,
                        record,
                    );
                    self.capture = CaptureState::Active(Box::new(session));
                    morph_to_overlay(ctx, bounds);
                    ctx.request_repaint();
                }
            }
            Err(err) => self.fail(ctx, err.to_string()),
        }
    }

    /// After the Timer countdown (and badge hide): grab the *live* desktop and
    /// produce the final image — the whole desktop, or the chosen region
    /// cropped/masked from the live grab.
    fn grab_live(&mut self, ctx: &egui::Context) {
        let (selection, markup) = match std::mem::replace(&mut self.capture, CaptureState::Idle) {
            CaptureState::GrabbingLive {
                selection, markup, ..
            } => (selection, markup),
            other => {
                self.capture = other;
                return;
            }
        };
        match capture_desktop() {
            Ok((composite, _windows)) => {
                let image = match &selection {
                    Some(sel) => apply_selection(&composite, sel),
                    None => Some(composite.into_image()),
                };
                match image {
                    Some(img) => self.deliver_or_edit(ctx, img, markup),
                    None => self.fail(ctx, "Selection produced no image.".to_owned()),
                }
            }
            Err(err) => self.fail(ctx, err.to_string()),
        }
    }

    /// Route a finished capture: open the image editor (Toolbar 2, P4.1) when
    /// Markup is armed, otherwise hand straight to delivery (clipboard + save)
    /// exactly as before.
    fn deliver_or_edit(&mut self, ctx: &egui::Context, image: RgbaImage, markup: bool) {
        // No editor, or an empty crop (let `finish` report it) → straight to save.
        if !markup || image.width() == 0 || image.height() == 0 {
            self.finish(ctx, Some(image));
            return;
        }
        let session = EditorSession::new(ctx, image, self.settings.active_color);
        // Reshape the single OS window into a decorated, centered editor window
        // (the immediate path arrives here as the full-desktop overlay; the timed
        // path arrives from a hidden window).
        morph_to_editor(ctx);
        self.capture = CaptureState::Editing(Box::new(session));
        ctx.request_repaint();
    }

    /// Draw the overlay into the (now full-desktop) window and act on the result.
    fn overlay_ui(&mut self, ui: &mut egui::Ui) {
        let (outcome, markup, new_color) = match &mut self.capture {
            CaptureState::Active(session) => {
                let mut out = session.ui(ui);
                if ui.input(|i| i.viewport().close_requested()) {
                    // A close request (e.g. Alt+F4) during a capture should cancel
                    // the snip, not quit the whole app — deny the close.
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    out = OverlayOutcome::Cancelled;
                }
                (out, session.markup(), session.active_color())
            }
            _ => return,
        };

        // Reflect a colour picked on the action bar so the choice carries to the
        // next snip + the home toolbar. Defer the disk write while the pointer is
        // held (dragging the colour-picker slider) and flush on release — exactly
        // like the home UI, so a drag never rewrites settings.json every frame.
        if self.settings.active_color != new_color {
            self.settings.active_color = new_color;
            self.needs_persist = true;
        }
        if self.needs_persist && !ui.input(|i| i.pointer.any_down()) {
            self.persist();
            self.needs_persist = false;
        }

        let ctx = ui.ctx().clone();
        match outcome {
            OverlayOutcome::Active => ctx.request_repaint(),
            OverlayOutcome::Cancelled => self.finish(&ctx, None),
            OverlayOutcome::Captured { image } => {
                self.deliver_or_edit(&ctx, image, markup);
            }
            // Timer set: selection chosen, now run the countdown then grab live.
            OverlayOutcome::Selected(selection) => {
                self.start_countdown(&ctx, Some(selection), markup)
            }
            // Video armed: begin recording the chosen geometry (P5.1).
            OverlayOutcome::RecordTarget(geom) => self.start_recording(&ctx, geom),
        }
    }

    /// Draw the post-capture editor (Toolbar 2 shell, P3.2) and act on Save /
    /// Discard. Hosted in the full-desktop overlay window, anchored below the
    /// selection.
    fn editor_ui(&mut self, ui: &mut egui::Ui) {
        let outcome = match &mut self.capture {
            CaptureState::Editing(session) => {
                let mut out = session.ui(ui);
                if ui.input(|i| i.viewport().close_requested()) {
                    // A close request while editing discards the capture rather than
                    // quitting the app.
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    out = EditorOutcome::Discard;
                }
                out
            }
            _ => return,
        };

        let ctx = ui.ctx().clone();
        match outcome {
            EditorOutcome::Active => {}
            EditorOutcome::Save => {
                if let CaptureState::Editing(session) =
                    std::mem::replace(&mut self.capture, CaptureState::Idle)
                {
                    self.finish(&ctx, Some(session.into_image()));
                }
            }
            // Copy the current image to the clipboard and keep editing — routed
            // through the delivery worker, which owns the clipboard for its life
            // (needed for X11/Wayland persistence).
            EditorOutcome::Copy => {
                if let CaptureState::Editing(session) = &self.capture {
                    self.delivery.copy(session.flatten());
                }
            }
            // OCR text → clipboard (P4.6b), via the same worker.
            EditorOutcome::CopyText(text) => {
                self.delivery.copy_text(text);
            }
            EditorOutcome::Discard => {
                self.finish(&ctx, None);
                self.status = Some("Capture discarded.".to_owned());
            }
        }
    }

    /// Close the overlay, restore the home window, and hand the capture to the
    /// background delivery worker (clipboard + save). `image == None` means the
    /// capture was cancelled. Returns immediately so the UI never blocks.
    fn finish(&mut self, ctx: &egui::Context, image: Option<RgbaImage>) {
        self.capture = CaptureState::Idle;
        restore_home(ctx, self.home_pos, !self.hidden_to_tray);
        match image {
            None => self.status = Some("Capture cancelled.".to_owned()),
            Some(img) if img.width() == 0 || img.height() == 0 => {
                self.status = Some("Capture was empty — nothing saved.".to_owned());
            }
            Some(img) => {
                self.status = Some(format!("Saving {} × {}…", img.width(), img.height()));
                self.delivery.deliver(
                    img,
                    self.settings.save_folder.clone(),
                    self.settings.default_image_format,
                );
            }
        }
        ctx.request_repaint();
    }

    /// Abort a capture after a backend failure, restoring the home window.
    fn fail(&mut self, ctx: &egui::Context, message: String) {
        self.capture = CaptureState::Idle;
        restore_home(ctx, self.home_pos, !self.hidden_to_tray);
        self.status = Some(format!("Capture failed: {message}"));
        ctx.request_repaint();
    }

    /// Start a screen recording (P5.1) of the chosen geometry: spawn the recorder
    /// worker and morph the window into a small control bar outside the recorded
    /// area.
    fn start_recording(&mut self, ctx: &egui::Context, geom: RecordGeom) {
        let (rect, follows_window, target) = match geom {
            RecordGeom::Rect(r) => (r, false, RecordTarget::Region(r)),
            RecordGeom::Window { id, bounds } => (
                bounds,
                true,
                RecordTarget::Window {
                    id,
                    initial: bounds,
                },
            ),
        };

        // Ensure the destination folder exists, then pick a unique .fvid path.
        let folder = self.settings.save_folder.clone();
        if let Err(err) = std::fs::create_dir_all(&folder) {
            self.fail(ctx, format!("Couldn't create the save folder: {err}"));
            return;
        }
        let output = crate::output::recording_path(&folder);

        let recorder = Recorder::start(
            ctx,
            RecordConfig {
                target,
                fps: self.settings.record_fps,
                output,
                audio: AudioConfig {
                    system: self.settings.record_system_audio,
                    mic: self.settings.record_microphone,
                },
                webcam: self.settings.record_webcam,
            },
        );
        self.capture = CaptureState::Recording(Box::new(Recording {
            recorder,
            rect,
            follows_window,
            elapsed: Duration::ZERO,
            paused: false,
            stopping: false,
        }));
        morph_to_record_bar(ctx, rect);
        self.status = None;
        ctx.request_repaint();
    }

    /// Poll the recorder for progress and finalize a finished recording.
    fn poll_recorder(&mut self, ctx: &egui::Context) {
        let mut finished: Option<std::result::Result<PathBuf, String>> = None;
        if let CaptureState::Recording(rec) = &mut self.capture {
            for update in rec.recorder.poll() {
                if let Some(result) = update.finished {
                    finished = Some(result);
                } else {
                    rec.elapsed = update.elapsed;
                    rec.paused = update.paused;
                }
            }
        }
        if let Some(result) = finished {
            self.finish_recording(ctx, result);
        }
    }

    /// A finished recording: restore the home window and record the saved file
    /// (or surface the error).
    fn finish_recording(
        &mut self,
        ctx: &egui::Context,
        result: std::result::Result<PathBuf, String>,
    ) {
        self.capture = CaptureState::Idle;
        match result {
            Ok(path) => {
                self.status = Some(format!("Saved recording to {}", path.display()));
                self.settings.push_recent(path);
                self.persist();
            }
            Err(message) => self.status = Some(format!("Recording failed: {message}")),
        }
        // If the user chose Quit during the recording, quit now that it's saved.
        if self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            restore_home(ctx, self.home_pos, !self.hidden_to_tray);
        }
        ctx.request_repaint();
    }

    /// Draw the recording control bar (REC · elapsed · Pause/Resume · Stop) in the
    /// small always-on-top window placed outside the recorded area.
    fn recording_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // A close request (Alt+F4) while recording stops + saves, rather than quits.
        let close = ui.input(|i| i.viewport().close_requested());
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

        // Snapshot the fields the UI needs, so the egui closures don't borrow `rec`.
        let (paused, elapsed, rect, follows_window, stopping) = match &self.capture {
            CaptureState::Recording(rec) => (
                rec.paused,
                rec.elapsed,
                rec.rect,
                rec.follows_window,
                rec.stopping,
            ),
            _ => return,
        };

        let mut stop_clicked = false;
        let mut pause_clicked = false;
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                // Blinking red dot (solid while paused).
                let blink = paused || (elapsed.as_millis() / 500).is_multiple_of(2);
                let (dot, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                if blink {
                    ui.painter().circle_filled(
                        dot.center(),
                        6.0,
                        egui::Color32::from_rgb(229, 57, 53),
                    );
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{}  {}",
                        if paused { "Paused" } else { "REC" },
                        format_hms(elapsed)
                    ))
                    .strong(),
                );
                let label = if follows_window {
                    "window".to_owned()
                } else {
                    format!("{} × {}", rect.width, rect.height)
                };
                ui.label(egui::RichText::new(label).weak().small());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(!stopping, egui::Button::new("⏹ Stop"))
                        .on_hover_text("Stop and save the recording")
                        .clicked()
                    {
                        stop_clicked = true;
                    }
                    if ui
                        .add_enabled(
                            !stopping,
                            egui::Button::new(if paused { "▶ Resume" } else { "⏸ Pause" }),
                        )
                        .on_hover_text("Pause / resume the recording")
                        .clicked()
                    {
                        pause_clicked = true;
                    }
                });
            });
            if stopping {
                ui.label(egui::RichText::new("Saving…").italics().weak());
            }
        });

        // Apply the chosen action now that the egui borrows are released.
        if let CaptureState::Recording(rec) = &mut self.capture {
            if stop_clicked || close {
                rec.recorder.stop();
                rec.stopping = true;
            } else if pause_clicked {
                if rec.paused {
                    rec.recorder.resume();
                } else {
                    rec.recorder.pause();
                }
            }
        }
    }

    /// Open a saved `.fvid` recording in the in-app player (P5.1).
    fn open_player(&mut self, ctx: &egui::Context, path: PathBuf) {
        match Player::open(ctx, path) {
            Ok(player) => {
                self.home_pos = ctx
                    .input(|i| i.viewport().outer_rect)
                    .map(|r| r.min)
                    .or(self.home_pos);
                morph_to_editor(ctx);
                self.capture = CaptureState::Playing(Box::new(player));
                self.status = None;
                ctx.request_repaint();
            }
            Err(message) => self.status = Some(format!("Couldn't play recording: {message}")),
        }
    }

    /// Draw the `.fvid` player and act on a Close / Edit request (P5.1).
    fn player_ui(&mut self, ui: &mut egui::Ui) {
        let outcome = match &mut self.capture {
            CaptureState::Playing(player) => player.ui(ui),
            _ => return,
        };
        match outcome {
            PlayerOutcome::Active => {}
            PlayerOutcome::Close => {
                let ctx = ui.ctx().clone();
                self.capture = CaptureState::Idle;
                restore_home(&ctx, self.home_pos, !self.hidden_to_tray);
                ctx.request_repaint();
            }
            PlayerOutcome::Edit(path) => {
                let ctx = ui.ctx().clone();
                self.open_timeline(&ctx, path);
            }
        }
    }

    /// Open a saved `.fvid` recording in the Phase 6 timeline editor (P6.1).
    fn open_timeline(&mut self, ctx: &egui::Context, path: PathBuf) {
        match TimelineEditor::from_recording(ctx, path) {
            Ok(editor) => {
                self.home_pos = ctx
                    .input(|i| i.viewport().outer_rect)
                    .map(|r| r.min)
                    .or(self.home_pos);
                morph_to_editor(ctx);
                self.capture = CaptureState::Timeline(Box::new(editor));
                self.status = None;
                ctx.request_repaint();
            }
            Err(message) => self.status = Some(format!("Couldn't open the editor: {message}")),
        }
    }

    /// Draw the timeline editor and act on a Close request (P6.1).
    fn timeline_ui(&mut self, ui: &mut egui::Ui) {
        let outcome = match &mut self.capture {
            CaptureState::Timeline(editor) => editor.ui(ui),
            _ => return,
        };
        if matches!(outcome, TimelineOutcome::Close) {
            let ctx = ui.ctx().clone();
            self.capture = CaptureState::Idle;
            restore_home(&ctx, self.home_pos, !self.hidden_to_tray);
            ctx.request_repaint();
        }
    }

    /// Draw the on-screen countdown badge (Timer ▾). Esc (or closing it) cancels
    /// the pending capture and restores the home window.
    fn countdown_ui(&mut self, ui: &mut egui::Ui) {
        // Esc only reaches us if the badge happens to be focused; it intentionally
        // is NOT (so the user can arrange the screen during the countdown), so a
        // click anywhere on the badge is the reliable cancel.
        let key_cancel =
            ui.input(|i| i.key_pressed(egui::Key::Escape) || i.viewport().close_requested());

        let seconds = match &self.capture {
            CaptureState::Counting { since, total, .. } => {
                total.saturating_sub(since.elapsed()).as_secs_f32().ceil() as u64
            }
            _ => return,
        }
        .max(1);

        let mut click_cancel = false;
        egui::CentralPanel::default().show_inside(ui, |ui| {
            click_cancel = ui
                .interact(
                    ui.max_rect(),
                    ui.id().with("countdown"),
                    egui::Sense::click(),
                )
                .clicked();
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label(egui::RichText::new(seconds.to_string()).size(64.0).strong());
                ui.label(egui::RichText::new("click or Esc to cancel").small().weak());
            });
        });

        if key_cancel || click_cancel {
            let ctx = ui.ctx().clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.finish(&ctx, None);
            return;
        }
        // The digit changes once a second; repaint a few times a second (the badge
        // is visible, so request_repaint_after is honoured) instead of at full rate.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(200));
    }

    /// The Win11-style home window: a persistent toolbar plus one of the
    /// Capture / Settings / About views.
    fn home_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut dirty = false;

        egui::Panel::top("toolbar").show_inside(ui, |ui| {
            self.toolbar(ui, &ctx, &mut dirty);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| match self.view {
            HomeView::Capture => self.capture_view(ui),
            HomeView::Settings => self.settings_view(ui, &ctx, &mut dirty),
            HomeView::About => self.about_view(ui),
        });

        // Coalesce writes: mark dirty now, but defer the disk write while the
        // pointer is down (e.g. dragging the colour picker) and flush once on
        // release — otherwise a drag rewrites settings.json every frame.
        self.needs_persist |= dirty;
        if self.needs_persist && !ui.input(|i| i.pointer.any_down()) {
            self.persist();
            self.needs_persist = false;
        }
    }

    /// The always-visible toolbar (P2.1): capture controls on the left,
    /// navigation + theme on the right.
    fn toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, dirty: &mut bool) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading("Freally Snipper");
            ui.separator();

            if ui
                .button("+ New")
                .on_hover_text("Start a capture in the selected snippet mode (after the timer)")
                .clicked()
            {
                self.begin_capture(ctx, self.settings.default_snippet_mode, false);
            }
            if ui
                .button("Camera")
                .on_hover_text("Take a screenshot (photo)")
                .clicked()
            {
                self.begin_capture(ctx, self.settings.default_snippet_mode, false);
            }
            if ui
                .button("Video")
                .on_hover_text("Record the screen (region / window / full screen) to a .fvid")
                .clicked()
            {
                self.begin_capture(ctx, self.settings.default_snippet_mode, true);
            }

            ui.separator();

            // Snippet ▾ — choose what + New and the hotkey capture.
            ui.menu_button(
                format!("Snippet: {}", self.settings.default_snippet_mode.label()),
                |ui| {
                    for mode in SnippetMode::ALL {
                        if ui
                            .selectable_value(
                                &mut self.settings.default_snippet_mode,
                                mode,
                                mode.label(),
                            )
                            .clicked()
                        {
                            *dirty = true;
                            ui.close();
                        }
                    }
                },
            )
            .response
            .on_hover_text("What + New and the hotkey capture");

            // Timer ▾ — delay before the capture starts.
            ui.menu_button(
                format!("Timer: {}", self.settings.timer_delay.label()),
                |ui| {
                    for delay in TimerDelay::ALL {
                        if ui
                            .selectable_value(&mut self.settings.timer_delay, delay, delay.label())
                            .clicked()
                        {
                            *dirty = true;
                            ui.close();
                        }
                    }
                },
            )
            .response
            .on_hover_text("Delay before the capture starts");

            // Color — the active markup colour (used by the editor's tools).
            ui.label("Color");
            if ui
                .color_edit_button_srgba_unmultiplied(&mut self.settings.active_color)
                .on_hover_text("Markup colour for the editor's tools")
                .changed()
            {
                *dirty = true;
            }

            // Right-aligned navigation + theme toggle.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (next, action) = match self.settings.theme {
                    Theme::Dark => (Theme::Light, "Light theme"),
                    Theme::Light => (Theme::Dark, "Dark theme"),
                };
                if ui
                    .button(action)
                    .on_hover_text("Toggle light/dark theme")
                    .clicked()
                {
                    self.settings.theme = next;
                    apply_theme(ui.ctx(), self.settings.theme);
                    *dirty = true;
                }
                ui.separator();
                if ui
                    .selectable_label(self.view == HomeView::About, "About")
                    .clicked()
                {
                    self.view = toggle_view(self.view, HomeView::About);
                }
                if ui
                    .selectable_label(self.view == HomeView::Settings, "Settings")
                    .clicked()
                {
                    self.view = toggle_view(self.view, HomeView::Settings);
                }
            });
        });
        ui.add_space(4.0);
    }

    /// The Capture view: the hint, the last-action status, and the recent strip.
    fn capture_view(&mut self, ui: &mut egui::Ui) {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "Press   {}   to start a snip",
                    self.settings.hotkey
                ))
                .size(22.0)
                .strong(),
            );
            ui.add_space(4.0);
            let extra = if self.settings.timer_delay == TimerDelay::None {
                String::new()
            } else {
                format!(" (waits {} first)", self.settings.timer_delay.label())
            };
            ui.label(format!(
                "…or click  + New  above{extra}. Esc cancels a capture."
            ));
        });

        ui.add_space(10.0);
        if let Some(status) = &self.status {
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(status).italics());
            });
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(6.0);
        ui.heading("Recent captures");
        ui.add_space(6.0);
        self.recent_strip(ui);
    }

    /// Horizontal strip of recent-capture thumbnails (P2.2). Clicking a tile
    /// opens it in the OS default viewer; right-click offers Open / Show in
    /// folder / Remove.
    fn recent_strip(&mut self, ui: &mut egui::Ui) {
        if self.settings.recent_captures.is_empty() {
            ui.label(egui::RichText::new("Your recent captures will appear here.").weak());
            return;
        }

        let recents = self.settings.recent_captures.clone();
        let mut to_open: Option<PathBuf> = None;
        let mut to_edit: Option<PathBuf> = None;
        let mut to_reveal: Option<PathBuf> = None;
        let mut to_remove: Option<PathBuf> = None;

        egui::ScrollArea::horizontal()
            .id_salt("recent_strip")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for path in &recents {
                        let tile = ui.allocate_ui(egui::vec2(146.0, TILE + 30.0), |ui| {
                            if draw_thumb(&mut self.gallery, ui, path) {
                                to_open = Some(path.clone());
                            }
                        });
                        tile.response.context_menu(|ui| {
                            if ui.button("Open").clicked() {
                                to_open = Some(path.clone());
                                ui.close();
                            }
                            if is_fvid(path) && ui.button("Edit (timeline)").clicked() {
                                to_edit = Some(path.clone());
                                ui.close();
                            }
                            if ui.button("Open folder").clicked() {
                                to_reveal = Some(path.clone());
                                ui.close();
                            }
                            if ui.button("Remove from list").clicked() {
                                to_remove = Some(path.clone());
                                ui.close();
                            }
                        });
                    }
                });
            });

        if let Some(path) = to_open {
            if is_fvid(&path) {
                let ctx = ui.ctx().clone();
                self.open_player(&ctx, path);
            } else if let Err(err) = opener::open(&path) {
                self.status = Some(format!("Couldn't open {}: {err}", path.display()));
            }
        }
        if let Some(path) = to_edit {
            let ctx = ui.ctx().clone();
            self.open_timeline(&ctx, path);
        }
        if let Some(path) = to_reveal {
            // Open the containing folder (default `opener::open` — no extra feature/deps).
            let folder = path.parent().unwrap_or(path.as_path());
            if let Err(err) = opener::open(folder) {
                self.status = Some(format!(
                    "Couldn't open folder for {}: {err}",
                    path.display()
                ));
            }
        }
        if let Some(path) = to_remove {
            self.settings.recent_captures.retain(|p| p != &path);
            self.persist();
        }
    }

    /// The Settings view (P2.2 + P1.5).
    fn settings_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, dirty: &mut bool) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                self.view = HomeView::Capture;
            }
            ui.heading("Settings");
        });
        ui.add_space(8.0);

        egui::ScrollArea::vertical()
            .id_salt("settings_scroll")
            .show(ui, |ui| {
                egui::Grid::new("settings_grid")
                    .num_columns(2)
                    .spacing([16.0, 10.0])
                    .show(ui, |ui| {
                        ui.label("Capture hotkey");
                        egui::ComboBox::from_id_salt("capture_hotkey")
                            .selected_text(self.settings.hotkey.clone())
                            .show_ui(ui, |ui| {
                                for &preset in crate::settings::HOTKEY_PRESETS {
                                    if ui
                                        .selectable_value(
                                            &mut self.settings.hotkey,
                                            preset.to_owned(),
                                            preset,
                                        )
                                        .changed()
                                    {
                                        *dirty = true;
                                        if let Some(h) = self.hotkeys.as_mut() {
                                            if !h.set_hotkey(preset) {
                                                self.status = Some(format!(
                                                    "Hotkey \"{preset}\" is in use by another app — pick another."
                                                ));
                                            }
                                        }
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label("Capture timer");
                        egui::ComboBox::from_id_salt("timer_delay")
                            .selected_text(self.settings.timer_delay.label())
                            .show_ui(ui, |ui| {
                                for delay in TimerDelay::ALL {
                                    if ui
                                        .selectable_value(&mut self.settings.timer_delay, delay, delay.label())
                                        .changed()
                                    {
                                        *dirty = true;
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label("Default snippet mode");
                        egui::ComboBox::from_id_salt("default_snippet_mode")
                            .selected_text(self.settings.default_snippet_mode.label())
                            .show_ui(ui, |ui| {
                                for mode in SnippetMode::ALL {
                                    if ui
                                        .selectable_value(
                                            &mut self.settings.default_snippet_mode,
                                            mode,
                                            mode.label(),
                                        )
                                        .changed()
                                    {
                                        *dirty = true;
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label("Default image format");
                        egui::ComboBox::from_id_salt("default_image_format")
                            .selected_text(self.settings.default_image_format.label())
                            .show_ui(ui, |ui| {
                                for format in ImageFormat::ALL {
                                    if ui
                                        .selectable_value(
                                            &mut self.settings.default_image_format,
                                            format,
                                            format.label(),
                                        )
                                        .changed()
                                    {
                                        *dirty = true;
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label("Theme");
                        egui::ComboBox::from_id_salt("theme")
                            .selected_text(match self.settings.theme {
                                Theme::Light => "Light",
                                Theme::Dark => "Dark",
                            })
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_value(&mut self.settings.theme, Theme::Light, "Light")
                                    .changed()
                                {
                                    apply_theme(ctx, self.settings.theme);
                                    *dirty = true;
                                }
                                if ui
                                    .selectable_value(&mut self.settings.theme, Theme::Dark, "Dark")
                                    .changed()
                                {
                                    apply_theme(ctx, self.settings.theme);
                                    *dirty = true;
                                }
                            });
                        ui.end_row();

                        ui.label("UI language");
                        egui::ComboBox::from_id_salt("ui_language")
                            .selected_text(language_native(&self.settings.ui_language))
                            .show_ui(ui, |ui| {
                                for (code, _english, native) in UI_LANGUAGES {
                                    if ui
                                        .selectable_value(
                                            &mut self.settings.ui_language,
                                            (*code).to_owned(),
                                            *native,
                                        )
                                        .changed()
                                    {
                                        *dirty = true;
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label("Save folder");
                        ui.horizontal(|ui| {
                            if ui
                                .button("Change…")
                                .on_hover_text("Choose where captures are saved")
                                .clicked()
                            {
                                let mut dialog = rfd::FileDialog::new();
                                if self.settings.save_folder.is_dir() {
                                    dialog = dialog.set_directory(&self.settings.save_folder);
                                } else if let Some(parent) =
                                    self.settings.save_folder.parent().filter(|p| p.is_dir())
                                {
                                    dialog = dialog.set_directory(parent);
                                }
                                if let Some(folder) = dialog.pick_folder() {
                                    self.settings.save_folder = folder;
                                    *dirty = true;
                                }
                            }
                            ui.label(self.settings.save_folder.display().to_string());
                        });
                        ui.end_row();
                    });

                ui.add_space(6.0);
                ui.small("UI translation arrives in Phase 7; selecting a language here saves your choice.");

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);
                ui.heading("Capture");
                ui.add_space(4.0);
                let mut show_editor = self.settings.show_capture_editor;
                if ui
                    .checkbox(&mut show_editor, "Show the capture editor after capturing")
                    .on_hover_text(
                        "Open each capture in the image editor (Toolbar 2 — markup, text, shapes, \
                         emoji, filters, transforms, OCR, and translation) instead of saving \
                         straight away. You can also toggle this per-capture with Markup on the \
                         capture bar.",
                    )
                    .changed()
                {
                    self.settings.show_capture_editor = show_editor;
                    *dirty = true;
                }
                ui.small(
                    "The editor opens in its own window — Save writes exactly what you see \
                     (Save / Copy / Discard, Undo / Redo).",
                );

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);
                ui.heading("Recording");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Frame rate");
                    egui::ComboBox::from_id_salt("record_fps")
                        .selected_text(format!("{} fps", self.settings.record_fps))
                        .show_ui(ui, |ui| {
                            for &fps in crate::settings::RECORD_FPS_OPTIONS {
                                if ui
                                    .selectable_value(
                                        &mut self.settings.record_fps,
                                        fps,
                                        format!("{fps} fps"),
                                    )
                                    .changed()
                                {
                                    *dirty = true;
                                }
                            }
                        });
                });
                let mut system_audio = self.settings.record_system_audio;
                if ui
                    .checkbox(&mut system_audio, "Capture system audio (what you hear)")
                    .on_hover_text(
                        "Record desktop/app sound. Windows: WASAPI loopback; Linux: a \
                         PulseAudio/PipeWire monitor source; macOS needs a virtual device \
                         (e.g. BlackHole). Best-effort — recording continues silently if it \
                         can't open.",
                    )
                    .changed()
                {
                    self.settings.record_system_audio = system_audio;
                    *dirty = true;
                }
                let mut microphone = self.settings.record_microphone;
                if ui
                    .checkbox(&mut microphone, "Capture microphone")
                    .on_hover_text("Mix your microphone into the recording (e.g. to narrate).")
                    .changed()
                {
                    self.settings.record_microphone = microphone;
                    *dirty = true;
                }
                let mut webcam = self.settings.record_webcam;
                if ui
                    .checkbox(&mut webcam, "Overlay webcam (picture-in-picture)")
                    .on_hover_text(
                        "Show your camera in a small box in the bottom-right of the recording. \
                         Best-effort — skipped if no camera is available.",
                    )
                    .changed()
                {
                    self.settings.record_webcam = webcam;
                    *dirty = true;
                }
                ui.small(
                    "Recordings save as .fvid (your own format) — click one in Recent captures \
                     to play it. The owned codec is lossless, so very high frame rates at 4K may \
                     not sustain on every machine; playback always stays the right length.",
                );

                ui.add_space(10.0);
                let tray_available = self.tray.is_some();
                let mut tray_on = self.settings.minimize_to_tray;
                if ui
                    .add_enabled(
                        tray_available,
                        egui::Checkbox::new(&mut tray_on, "Minimize to system tray"),
                    )
                    .on_hover_text(
                        "Keep Freally Snipper in the system tray when the window is closed, so the \
                         hotkey and Print Screen keep working. Double-click the tray icon (or its \
                         menu) to reopen; Quit exits.",
                    )
                    .changed()
                {
                    self.settings.minimize_to_tray = tray_on;
                    if let Some(tray) = &self.tray {
                        tray.set_visible(tray_on);
                    }
                    *dirty = true;
                }
                if !tray_available {
                    ui.small("System tray runs on Windows and macOS; Linux support arrives in Phase 7.");
                }

                // Start at login (P4.10) — a reversible per-user autostart entry.
                ui.add_space(8.0);
                let mut start_at_login = self.settings.start_at_login;
                if ui
                    .checkbox(&mut start_at_login, "Start Freally Snipper when I sign in")
                    .on_hover_text(
                        "Launch at sign-in, minimized to the tray, so the hotkey / Print Screen \
                         work any time without opening the window. Reversible — NOT an OS service.",
                    )
                    .changed()
                {
                    match crate::autostart::apply(start_at_login) {
                        Ok(()) => {
                            self.settings.start_at_login = start_at_login;
                            *dirty = true;
                            self.status = Some(
                                if start_at_login {
                                    "Freally Snipper will start (minimized) when you sign in."
                                } else {
                                    "Removed start-at-login."
                                }
                                .to_owned(),
                            );
                        }
                        Err(err) => {
                            self.status = Some(format!("Couldn't update start-at-login: {err}"));
                        }
                    }
                }
                ui.small(
                    "Starts minimized to the tray. On Linux the tray arrives in Phase 7, so it \
                     starts hidden — reopen with the hotkey.",
                );

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);
                self.print_screen_section(ui);

                ui.add_space(12.0);
                let path = crate::settings::settings_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<unavailable>".to_owned());
                ui.small(format!("Settings file: {path}"));
            });
    }

    /// The Print-Screen override section (P1.5): a toggle plus per-OS guidance.
    fn print_screen_section(&mut self, ui: &mut egui::Ui) {
        ui.heading("Print Screen");
        ui.add_space(4.0);

        let mut enabled = self.settings.open_with_print_screen;
        if ui
            .checkbox(&mut enabled, "Open Freally Snipper with Print Screen")
            .on_hover_text("Use the Print Screen key to start a capture (opt-in, reversible)")
            .changed()
        {
            self.toggle_print_screen(enabled);
        }

        #[cfg(windows)]
        ui.small(
            "Windows: frees Print Screen from the built-in Snipping Tool (a Windows setting \
             Freally Snipper restores when you turn this off).",
        );
        #[cfg(target_os = "macos")]
        ui.small("macOS: the system screenshot shortcuts can't be overridden by an app — use the steps below.");
        #[cfg(all(not(windows), not(target_os = "macos")))]
        ui.small(
            "Linux: your desktop environment owns Print Screen — use the steps below to rebind it.",
        );

        if let Some((message, deep_link)) = self.print_screen_guidance.clone() {
            ui.add_space(6.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(message);
                if let Some(link) = deep_link {
                    if ui.button("Open System Settings").clicked() {
                        if let Err(err) = opener::open(&link) {
                            self.status = Some(format!("Couldn't open settings: {err}"));
                        }
                    }
                }
            });
        }
    }

    /// Apply (or revert) the Print-Screen override and reflect it in settings,
    /// the hotkey registration, and the status/guidance shown to the user.
    fn toggle_print_screen(&mut self, enabled: bool) {
        match print_screen::apply(enabled, &mut self.settings) {
            KeyOutcome::Applied(message) => {
                self.settings.open_with_print_screen = enabled;
                self.print_screen_guidance = None;
                // Register/unregister the key; if enabling but the OS won't grant
                // Print Screen (another app owns it), say so rather than claim success.
                let registered = self
                    .hotkeys
                    .as_mut()
                    .is_none_or(|h| h.set_print_screen(enabled));
                self.status = Some(if enabled && !registered {
                    "Freed the Print Screen key in Windows, but couldn't register it (another \
                     app may already use it). Pick a different hotkey or close that app."
                        .to_owned()
                } else {
                    message
                });
                self.persist();
            }
            KeyOutcome::Guidance { message, deep_link } => {
                self.settings.open_with_print_screen = enabled;
                if let Some(h) = self.hotkeys.as_mut() {
                    h.set_print_screen(enabled);
                }
                self.print_screen_guidance = enabled.then_some((message, deep_link));
                self.persist();
            }
            KeyOutcome::Declined => {
                // Setting is left unchanged; the checkbox reverts next frame.
                self.status = Some("Left the Print Screen key unchanged.".to_owned());
            }
            KeyOutcome::Failed(message) => {
                // Setting is left unchanged; surface the error.
                self.status = Some(message);
            }
        }
    }

    /// The About view (P2.2): version, ownership, dates, license + notices.
    fn about_view(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                self.view = HomeView::Capture;
            }
            ui.heading("About Freally Snipper");
        });
        ui.add_space(8.0);

        egui::ScrollArea::vertical()
            .id_salt("about_scroll")
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!("Freally Snipper v{}", env!("CARGO_PKG_VERSION")))
                        .size(20.0)
                        .strong(),
                );
                ui.add_space(2.0);
                ui.label("© Mike Weaver <mythodikalone@gmail.com> — All Rights Reserved");
                ui.label(egui::RichText::new("free · local-first · privacy-respecting").italics());
                ui.add_space(8.0);
                ui.label("Project started: June 16th, 2026 · 2:35 PM CDT");
                ui.label("v1.0.0 released: ______");
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                ui.collapsing("License", |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("license_scroll")
                        .max_height(220.0)
                        .show(ui, |ui| {
                            ui.monospace(LICENSE_TEXT);
                        });
                });
                ui.collapsing("Third-party notices", |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("third_party_scroll")
                        .max_height(260.0)
                        .show(ui, |ui| {
                            ui.monospace(THIRD_PARTY_TEXT);
                        });
                });
                ui.collapsing("End User License Agreement", |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("eula_scroll")
                        .max_height(260.0)
                        .show(ui, |ui| {
                            ui.monospace(EULA_TEXT);
                        });
                });
                ui.collapsing("Privacy", |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("privacy_scroll")
                        .max_height(260.0)
                        .show(ui, |ui| {
                            ui.monospace(PRIVACY_TEXT);
                        });
                });
            });
    }
}

impl eframe::App for FreallySnipperApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if matches!(self.capture, CaptureState::Active(_)) {
            self.overlay_ui(ui);
        } else if matches!(self.capture, CaptureState::Editing(_)) {
            self.editor_ui(ui);
        } else if matches!(self.capture, CaptureState::Counting { .. }) {
            self.countdown_ui(ui);
        } else if matches!(self.capture, CaptureState::Recording(_)) {
            self.recording_ui(ui);
        } else if matches!(self.capture, CaptureState::Playing(_)) {
            self.player_ui(ui);
        } else if matches!(self.capture, CaptureState::Timeline(_)) {
            self.timeline_ui(ui);
        } else {
            self.home_ui(ui);
        }
    }
}

/// Toggle `view` to `target`, or back to Capture if it is already showing.
fn toggle_view(view: HomeView, target: HomeView) -> HomeView {
    if view == target {
        HomeView::Capture
    } else {
        target
    }
}

/// Draw one recent-capture tile (thumbnail + its modified date/time), returning
/// `true` if it was clicked (to open). The date label and texture id + size are
/// read out first so the gallery borrow doesn't extend into the draw branches.
fn draw_thumb(gallery: &mut Gallery, ui: &mut egui::Ui, path: &Path) -> bool {
    let when = gallery.modified_label(path).to_owned();
    let texture = gallery.thumbnail(path).map(|t| (t.id(), t.size()));
    let failed = gallery.is_failed(path);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let hover = format!("{name}\n{when}\nClick to open in your default viewer");

    let mut clicked = false;
    ui.vertical_centered(|ui| {
        if let Some((id, [w, h])) = texture {
            // Uniform square tile showing the WHOLE image (letterboxed, never
            // cropped), so every tile is the same size regardless of aspect ratio.
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(TILE, TILE), egui::Sense::click());
            let response = response.on_hover_text(&hover);
            let widget = if response.hovered() {
                ui.visuals().widgets.hovered
            } else {
                ui.visuals().widgets.inactive
            };
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(4), widget.bg_fill);
            let fit = fit_within([w as f32, h as f32], TILE - 8.0);
            let image = egui::Image::from_texture(egui::load::SizedTexture::new(id, fit));
            image.paint_at(ui, egui::Rect::from_center_size(rect.center(), fit));
            clicked = response.clicked();
        } else if failed {
            clicked = ui
                .add_sized([TILE, TILE], egui::Button::new("Open"))
                .on_hover_text(&hover)
                .clicked();
        } else {
            ui.allocate_ui(egui::vec2(TILE, TILE), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                });
            });
        }
        ui.add(egui::Label::new(egui::RichText::new(&when).small().weak()).truncate());
    });
    clicked
}

/// Downscale the (already-square) app icon to a small, crisp `size`×`size` RGBA
/// for the system tray. Windows scales the tray slot to ~16–32px, so handing it
/// the full-resolution icon yields a distorted/blurry result; a clean Lanczos
/// downscale to a small square fixes it. Falls back to the original on error.
fn tray_icon_rgba(icon: &egui::IconData, size: u32) -> (Vec<u8>, u32, u32) {
    match image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba.clone()) {
        Some(img) => {
            let small =
                image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3);
            (small.into_raw(), size, size)
        }
        None => (icon.rgba.clone(), icon.width, icon.height),
    }
}

/// Scale `[w, h]` to fit within a `max`×`max` box, never upscaling — used to show
/// a whole thumbnail centered inside a uniform square tile.
fn fit_within([w, h]: [f32; 2], max: f32) -> egui::Vec2 {
    if w <= 0.0 || h <= 0.0 {
        return egui::vec2(max, max);
    }
    let scale = (max / w).min(max / h).min(1.0);
    egui::vec2(w * scale, h * scale)
}

/// Apply the light/dark theme preference to the egui context.
pub(crate) fn apply_theme(ctx: &egui::Context, theme: Theme) {
    let preference = match theme {
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    };
    ctx.set_theme(preference);
}

/// Size of the on-screen countdown badge, in points.
const BADGE_SIZE: egui::Vec2 = egui::vec2(150.0, 150.0);

/// Shrink the single OS window into a small, borderless, always-on-top countdown
/// badge centered on `center`. Focus is intentionally NOT taken, so the user can
/// still arrange the target window while the countdown runs.
fn morph_to_countdown_badge(ctx: &egui::Context, center: egui::Pos2) {
    let pos = center - BADGE_SIZE * 0.5;
    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
        egui::WindowLevel::AlwaysOnTop,
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(BADGE_SIZE));
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
}

/// Center of the monitor the window is on, in points (for placing the countdown
/// badge). Falls back to a reasonable default if the monitor size is unknown.
fn screen_center(ctx: &egui::Context) -> egui::Pos2 {
    ctx.input(|i| i.viewport().monitor_size)
        .map(|m| egui::pos2(m.x / 2.0, m.y / 2.0))
        .unwrap_or(egui::pos2(700.0, 450.0))
}

/// Reconfigure the single OS window to cover the whole virtual desktop as a
/// borderless, always-on-top selection overlay.
fn morph_to_overlay(ctx: &egui::Context, bounds: VRect) {
    let ppp = ctx.pixels_per_point().max(0.1);
    let pos = egui::pos2(bounds.x as f32 / ppp, bounds.y as f32 / ppp);
    let size = egui::vec2(bounds.width as f32 / ppp, bounds.height as f32 / ppp);
    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
        egui::WindowLevel::AlwaysOnTop,
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

/// Comfortable editor-window size (points) for the canvas + Toolbar 2, clamped so
/// it always fits on the monitor.
fn editor_size(ctx: &egui::Context) -> egui::Vec2 {
    let target = egui::vec2(1100.0, 760.0);
    match ctx.input(|i| i.viewport().monitor_size) {
        Some(m) => egui::vec2(target.x.min(m.x * 0.92), target.y.min(m.y * 0.92)),
        None => target,
    }
}

/// Reshape the single OS window into a decorated, centered editor window for the
/// Toolbar 2 image editor (P4.1). Reverses [`morph_to_overlay`]: a normal,
/// decorated, resizable window the user can move and resize while editing.
fn morph_to_editor(ctx: &egui::Context) {
    let size = editor_size(ctx);
    let pos = ctx
        .input(|i| i.viewport().monitor_size)
        .map(|m| egui::pos2((m.x - size.x) * 0.5, (m.y - size.y) * 0.5))
        .unwrap_or(egui::pos2(120.0, 80.0));
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
        egui::WindowLevel::Normal,
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

/// Restore the window's decorated home-window shape after a capture ends. `show`
/// is false when the app is minimized to the tray, so the window keeps its shape
/// but stays hidden instead of popping back up.
fn restore_home(ctx: &egui::Context, home_pos: Option<egui::Pos2>, show: bool) {
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
        egui::WindowLevel::Normal,
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(HOME_SIZE));
    if let Some(pos) = home_pos {
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(show));
    if show {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

/// Size of the recording control bar window, in points.
const RECORD_BAR_SIZE: egui::Vec2 = egui::vec2(340.0, 60.0);

/// Reshape the single OS window into a small, borderless, always-on-top recording
/// control bar, placed just outside the recorded `rect` so it stays out of the
/// shot. For a full-screen recording there is no "outside", so it sits at the top.
fn morph_to_record_bar(ctx: &egui::Context, rect: VRect) {
    let ppp = ctx.pixels_per_point().max(0.1);
    let (bw, bh) = (RECORD_BAR_SIZE.x, RECORD_BAR_SIZE.y);
    let rect_x = rect.x as f32 / ppp;
    let rect_y = rect.y as f32 / ppp;
    let rect_w = rect.width as f32 / ppp;
    let rect_bottom = rect.bottom() as f32 / ppp;
    let margin = 8.0;
    let mut x = rect_x + (rect_w - bw) * 0.5;
    // Prefer just above the region; if there's no room, drop just below it.
    let above = rect_y - bh - margin;
    let mut y = if above >= 0.0 {
        above
    } else {
        rect_bottom + margin
    };
    // Clamp fully on-screen so the bar (and its Stop button) is always reachable —
    // e.g. a full-screen recording, where there is no room outside the rect.
    if let Some(m) = ctx.input(|i| i.viewport().monitor_size) {
        x = x.clamp(0.0, (m.x - bw).max(0.0));
        y = y.clamp(0.0, (m.y - bh).max(0.0));
    } else {
        x = x.max(0.0);
        y = y.max(0.0);
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
        egui::WindowLevel::AlwaysOnTop,
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(RECORD_BAR_SIZE));
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

/// Format a duration as `M:SS` (or `H:MM:SS` past an hour) for the recording bar.
fn format_hms(d: Duration) -> String {
    let secs = d.as_secs();
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Whether `path` is one of our owned `.fvid` recordings (case-insensitive).
fn is_fvid(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("fvid"))
}

/// Grab the whole desktop as a frozen composite plus the front-to-back window
/// list (for Window mode).
fn capture_desktop() -> freally_capture::Result<(Composite, Vec<WindowInfo>)> {
    let monitors = freally_capture::capture_all()?;
    let composite =
        freally_capture::composite(&monitors).ok_or(freally_capture::CaptureError::NoMonitors)?;
    let windows = freally_capture::list_windows().unwrap_or_default();
    Ok((composite, windows))
}
