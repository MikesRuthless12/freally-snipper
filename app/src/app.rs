//! The application: the Win11-style home window plus the Phase 1 capture flow
//! (hide the window → grab the desktop → selection overlay → save the result).
//!
//! The capture state machine runs in [`eframe::App::logic`], which eframe calls
//! every frame *even while the home window is hidden* — so we can hide the home
//! chrome, grab a clean shot, and only then bring the window back as a
//! full-desktop selection overlay. Rendering happens in [`eframe::App::ui`]: the
//! home toolbar when idle, or the overlay (drawn into this same window) while a
//! capture is in progress. We reuse the one OS window rather than a child
//! viewport because immediate child viewports cannot render while their parent
//! is hidden.

use std::time::{Duration, Instant};

use eframe::egui;
use freally_capture::image::RgbaImage;
use freally_capture::{Composite, Rect as VRect, WindowInfo};

use crate::delivery::Delivery;
use crate::hotkey::Hotkeys;
use crate::overlay::{OverlayOutcome, OverlaySession};
use crate::settings::{ImageFormat, Settings, SnippetMode, Theme};

/// Delay between hiding the home window and grabbing the screen, so the window is
/// actually gone from the shot before the snapshot is taken.
const HIDE_DELAY: Duration = Duration::from_millis(150);

/// Default home-window size, restored after a capture.
const HOME_SIZE: egui::Vec2 = egui::vec2(900.0, 600.0);

pub struct FreallySnipperApp {
    settings: Settings,
    capture: CaptureState,
    /// Global capture hotkey, or `None` if the OS would not grant it.
    hotkeys: Option<Hotkeys>,
    /// Background worker that copies captures to the clipboard and saves them,
    /// so committing a capture never blocks the UI.
    delivery: Delivery,
    /// Home-window position remembered across a capture, to restore afterwards.
    home_pos: Option<egui::Pos2>,
    /// Last-action message shown on the home window (saved path / cancelled / error).
    status: Option<String>,
}

enum CaptureState {
    Idle,
    /// Home window hidden; waiting for it to disappear before the snapshot.
    Arming {
        mode: SnippetMode,
        since: Instant,
    },
    /// Overlay is live with a frozen snapshot.
    Active(Box<OverlaySession>),
}

impl FreallySnipperApp {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings) -> Self {
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
        }

        Self {
            settings,
            capture: CaptureState::Idle,
            hotkeys,
            delivery: Delivery::new(&cc.egui_ctx),
            home_pos: None,
            status,
        }
    }

    fn persist(&self) {
        if let Err(err) = self.settings.save() {
            eprintln!("Freally Snipper: could not save settings: {err}");
        }
    }

    /// Start a capture: remember the home position, hide the chrome, then arm.
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
        };
        ctx.request_repaint();
    }

    /// Advance the capture state machine. Called from `App::logic` every frame
    /// (including while the home window is hidden).
    fn tick(&mut self, ctx: &egui::Context) {
        // Reflect a finished background delivery (clipboard + save) in the status.
        if let Some(message) = self.delivery.poll_status() {
            self.status = Some(message);
        }

        // A global-hotkey press opens the overlay (only honored while idle).
        let hotkey_fired = self.hotkeys.as_ref().is_some_and(Hotkeys::take_fired);
        if hotkey_fired {
            self.begin_capture(ctx, self.settings.default_snippet_mode);
        }

        match &self.capture {
            CaptureState::Idle => {}
            CaptureState::Arming { mode, since } => {
                let mode = *mode;
                if since.elapsed() < HIDE_DELAY {
                    ctx.request_repaint();
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

    /// The Win11-style home window (toolbar, hint, status, settings).
    fn home_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut dirty = false;

        egui::Panel::top("toolbar").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("Freally Snipper");
                ui.separator();

                if ui
                    .button("+ New")
                    .on_hover_text("Start a capture in the selected snippet mode")
                    .clicked()
                {
                    self.begin_capture(&ctx, self.settings.default_snippet_mode);
                }

                if ui
                    .button("Camera")
                    .on_hover_text("Capture a window")
                    .clicked()
                {
                    self.begin_capture(&ctx, SnippetMode::Window);
                }

                ui.add_enabled(false, egui::Button::new("Video"))
                    .on_disabled_hover_text("Screen recording — arrives in Phase 5");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (next, action) = match self.settings.theme {
                        Theme::Dark => (Theme::Light, "Switch to light theme"),
                        Theme::Light => (Theme::Dark, "Switch to dark theme"),
                    };
                    if ui.button(action).clicked() {
                        self.settings.theme = next;
                        apply_theme(ui.ctx(), self.settings.theme);
                        dirty = true;
                    }
                });
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(format!("Press   {}   to start a snip", self.settings.hotkey))
                        .size(22.0)
                        .strong(),
                );
                ui.add_space(4.0);
                ui.label("…or click  + New  above. Esc cancels a capture.");
            });

            ui.add_space(12.0);
            if let Some(status) = &self.status {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(status).italics());
                });
            }

            ui.add_space(16.0);
            ui.separator();
            ui.heading("Settings");
            ui.add_space(6.0);

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
                                    dirty = true;
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
                                    dirty = true;
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
                                    dirty = true;
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
                            // Open the native folder picker at the current folder,
                            // else its parent (the Pictures dir for the default).
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
                                dirty = true;
                            }
                        }
                        ui.label(self.settings.save_folder.display().to_string());
                    });
                    ui.end_row();
                });

            ui.add_space(10.0);
            let path = crate::settings::settings_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<unavailable>".to_owned());
            ui.small(format!("Settings file: {path}"));
        });

        if dirty {
            self.persist();
        }
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
