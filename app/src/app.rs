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

use crate::delivery::Delivery;
use crate::gallery::Gallery;
use crate::hotkey::Hotkeys;
use crate::overlay::{OverlayOutcome, OverlaySession};
use crate::print_screen::{self, KeyOutcome};
use crate::settings::{
    language_label, ImageFormat, Settings, SnippetMode, Theme, TimerDelay, UI_LANGUAGES,
};

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
}

enum CaptureState {
    Idle,
    /// Home window hidden; waiting `delay` (hide settle + Timer ▾) before the snapshot.
    Arming {
        mode: SnippetMode,
        since: Instant,
        delay: Duration,
    },
    /// Overlay is live with a frozen snapshot.
    Active(Box<OverlaySession>),
}

impl FreallySnipperApp {
    pub fn new(cc: &eframe::CreationContext<'_>, mut settings: Settings) -> Self {
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
            if settings.open_with_print_screen {
                h.set_print_screen(true);
            }
        }

        // Drop recents whose files were deleted since last run.
        let before = settings.recent_captures.len();
        settings.prune_recent();
        let pruned = settings.recent_captures.len() != before;

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
    /// (hide settle + the Timer ▾ delay) before the snapshot is taken.
    fn begin_capture(&mut self, ctx: &egui::Context, mode: SnippetMode) {
        if !matches!(self.capture, CaptureState::Idle) {
            return;
        }
        self.status = None;
        self.home_pos = ctx.input(|i| i.viewport().outer_rect).map(|r| r.min);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        self.capture = CaptureState::Arming {
            mode,
            since: Instant::now(),
            delay: HIDE_DELAY + self.settings.timer_delay.duration(),
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

        // A global-hotkey press opens the overlay (only honored while idle).
        let hotkey_fired = self.hotkeys.as_ref().is_some_and(Hotkeys::take_fired);
        if hotkey_fired {
            self.begin_capture(ctx, self.settings.default_snippet_mode);
        }

        match &self.capture {
            CaptureState::Idle => {}
            CaptureState::Arming { mode, since, delay } => {
                let mode = *mode;
                if since.elapsed() < *delay {
                    // Sleep until the snapshot is due instead of busy-repainting
                    // every frame through the (up to 10 s) Timer countdown.
                    ctx.request_repaint_after(*delay - since.elapsed());
                    return;
                }
                match capture_desktop() {
                    Ok((composite, windows)) => {
                        if mode == SnippetMode::FullScreen {
                            // No overlay needed — the whole desktop is the capture
                            // (move the stitched image out, no extra copy).
                            self.finish(ctx, Some(composite.into_image()));
                        } else {
                            let bounds = composite.bounds;
                            let session = OverlaySession::new(ctx, composite, mode, windows);
                            self.capture = CaptureState::Active(Box::new(session));
                            morph_to_overlay(ctx, bounds);
                            ctx.request_repaint();
                        }
                    }
                    Err(err) => self.fail(ctx, err.to_string()),
                }
            }
            CaptureState::Active(_) => {
                // Rendering + input happen in `ui`; just keep frames flowing.
                ctx.request_repaint();
            }
        }
    }

    /// Draw the overlay into the (now full-desktop) window and act on the result.
    fn overlay_ui(&mut self, ui: &mut egui::Ui) {
        let outcome = match &mut self.capture {
            CaptureState::Active(session) => {
                let mut out = session.ui(ui);
                if ui.input(|i| i.viewport().close_requested()) {
                    // A close request (e.g. Alt+F4) during a capture should cancel
                    // the snip, not quit the whole app — deny the close.
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    out = OverlayOutcome::Cancelled;
                }
                out
            }
            _ => return,
        };

        let ctx = ui.ctx().clone();
        match outcome {
            OverlayOutcome::Active => ctx.request_repaint(),
            OverlayOutcome::Cancelled => self.finish(&ctx, None),
            OverlayOutcome::Captured(img) => self.finish(&ctx, Some(img)),
        }
    }

    /// Close the overlay, restore the home window, and hand the capture to the
    /// background delivery worker (clipboard + save). `image == None` means the
    /// capture was cancelled. Returns immediately so the UI never blocks.
    fn finish(&mut self, ctx: &egui::Context, image: Option<RgbaImage>) {
        self.capture = CaptureState::Idle;
        restore_home(ctx, self.home_pos);
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
        restore_home(ctx, self.home_pos);
        self.status = Some(format!("Capture failed: {message}"));
        ctx.request_repaint();
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
                self.begin_capture(ctx, self.settings.default_snippet_mode);
            }
            if ui
                .button("Camera")
                .on_hover_text("Take a screenshot (photo)")
                .clicked()
            {
                self.begin_capture(ctx, self.settings.default_snippet_mode);
            }
            ui.add_enabled(false, egui::Button::new("Video"))
                .on_disabled_hover_text("Screen recording — arrives in Phase 5");

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
    /// opens it in the OS default viewer (the in-app editor arrives in Phase 4);
    /// right-click offers Open / Show in folder / Remove.
    fn recent_strip(&mut self, ui: &mut egui::Ui) {
        if self.settings.recent_captures.is_empty() {
            ui.label(egui::RichText::new("Your recent captures will appear here.").weak());
            return;
        }

        let recents = self.settings.recent_captures.clone();
        let mut to_open: Option<PathBuf> = None;
        let mut to_reveal: Option<PathBuf> = None;
        let mut to_remove: Option<PathBuf> = None;

        egui::ScrollArea::horizontal()
            .id_salt("recent_strip")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for path in &recents {
                        let tile = ui.allocate_ui(egui::vec2(TILE + 8.0, TILE + 8.0), |ui| {
                            if draw_thumb(&mut self.gallery, ui, path) {
                                to_open = Some(path.clone());
                            }
                        });
                        tile.response.context_menu(|ui| {
                            if ui.button("Open").clicked() {
                                to_open = Some(path.clone());
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
            if let Err(err) = opener::open(&path) {
                self.status = Some(format!("Couldn't open {}: {err}", path.display()));
            }
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
                            .selected_text(language_label(&self.settings.ui_language))
                            .show_ui(ui, |ui| {
                                for (code, english, _native) in UI_LANGUAGES {
                                    if ui
                                        .selectable_value(
                                            &mut self.settings.ui_language,
                                            (*code).to_owned(),
                                            *english,
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

/// Draw one recent-capture tile, returning `true` if it was clicked (to open).
/// Texture id + size are copied out first so the gallery borrow doesn't extend
/// into the `else` branches.
fn draw_thumb(gallery: &mut Gallery, ui: &mut egui::Ui, path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());

    let texture = gallery.thumbnail(path).map(|t| (t.id(), t.size()));
    let failed = gallery.is_failed(path);

    if let Some((id, [w, h])) = texture {
        let size = fit_within([w as f32, h as f32], TILE);
        let image = egui::Image::from_texture(egui::load::SizedTexture::new(id, size));
        ui.add(egui::Button::image(image))
            .on_hover_text(format!(
                "{name}\nOpen (the in-app editor arrives in Phase 4)"
            ))
            .clicked()
    } else if failed {
        ui.add_sized([TILE, TILE], egui::Button::new("Open"))
            .on_hover_text(format!("{name}\nPreview unavailable"))
            .clicked()
    } else {
        ui.allocate_ui(egui::vec2(TILE, TILE), |ui| {
            ui.centered_and_justified(|ui| {
                ui.spinner();
            });
        });
        false
    }
}

/// Scale `[w, h]` to fit within a `max`×`max` box, never upscaling.
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

/// Restore the window to the decorated home window after a capture ends.
fn restore_home(ctx: &egui::Context, home_pos: Option<egui::Pos2>) {
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
        egui::WindowLevel::Normal,
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(HOME_SIZE));
    if let Some(pos) = home_pos {
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
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
