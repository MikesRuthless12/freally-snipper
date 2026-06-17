//! `freally-editor` — the WYSIWYG image editor (Toolbar 2) for Freally Snipper.
//!
//! Phase 4 lives here. The guiding rule is *"Save writes exactly what you see."*
//!
//! - **P4.1** — the surface: the capture on a **zoom/pan canvas**, the **Toolbar 2**
//!   strip, and **Undo / Redo / Save / Copy / Discard**.
//! - **P4.2** — the **raster tools**: Pen, Brush, Highlighter (free + text-aware),
//!   and a two-mode Eraser (erase-to-white / restore-original). Each has its own
//!   adjustable **size**. Strokes preview live and bake into the raster on release;
//!   every bake is a single undo step.
//!
//! Still to come on the same [`EditorSession`]: movable text / shape / watermark /
//! emoji / image objects (P4.3, P4.7, P4.8); live filters (P4.5); transforms +
//! eyedropper + OCR (P4.6); translate-as-you-type text (P4.9). Until each lands,
//! its toolbar button is present but disabled and labelled with the prompt that
//! enables it — the capture bar's convention for not-yet-built features.
//!
//! The editor is drawn into the app's single OS window (morphed to a decorated
//! editor window), matching the one-window model the capture overlay already uses.

mod raster;

use egui::{Color32, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use freally_capture::image::RgbaImage;

use raster::Paint;

/// Crate identifier, surfaced in version banners and logs.
pub const CRATE_NAME: &str = "freally-editor";

/// Zoom limits (points per image pixel): from a small overview to pixel-level.
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 16.0;
/// Keep at least this many points of the image inside the canvas when panning, so
/// the picture can never be dragged completely out of view (foolproof controls).
const PAN_KEEP: f32 = 40.0;

/// Adjustable tool size range, in image pixels (the size slider's bounds).
const MIN_WIDTH: f32 = 1.0;
const MAX_WIDTH: f32 = 200.0;
/// Translucency of a highlighter stroke.
const HL_ALPHA: f32 = 0.4;
/// Undo depth. Each step is a full-image snapshot, so this also bounds memory.
const MAX_UNDO: usize = 24;

/// What the editor wants the host app to do after a UI frame.
pub enum EditorOutcome {
    /// Keep editing — nothing to do.
    Active,
    /// Flatten and save to the folder + copy to the clipboard, then return home.
    Save,
    /// Copy the current (flattened) image to the clipboard; keep editing.
    Copy,
    /// Throw the capture away and return home.
    Discard,
}

/// The active markup tool.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    /// Move/zoom the view (no drawing) — the default.
    Pan,
    Pen,
    Brush,
    Highlighter,
    Eraser,
}

impl Tool {
    /// Whether this tool paints onto the raster (vs. just panning the view).
    fn is_drawing(self) -> bool {
        !matches!(self, Tool::Pan)
    }
}

/// Highlighter behaviour.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HlMode {
    /// Translucent stroke over anything.
    Free,
    /// Highlight only detected text within the stroke band.
    TextAware,
}

/// Eraser behaviour.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EraseMode {
    /// Paint opaque white.
    White,
    /// Restore the original captured pixels (remove markup only).
    MarkupOnly,
}

/// A live editing session over one captured image.
pub struct EditorSession {
    /// The working raster — exactly what Save writes. Markup bakes here.
    image: RgbaImage,
    /// A pristine copy of the original capture, for the markup-only eraser.
    pristine: RgbaImage,
    /// GPU texture mirroring `image`, re-uploaded whenever the raster changes.
    texture: egui::TextureHandle,
    /// Zoom + pan of the canvas view.
    view: View,
    /// Active tool + its parameters.
    tool: Tool,
    /// Active markup colour (RGBA), seeded from the capture bar's colour.
    color: [u8; 4],
    /// Per-tool stroke widths (image pixels), so each tool remembers its size.
    pen_width: f32,
    brush_width: f32,
    hl_width: f32,
    eraser_width: f32,
    hl_mode: HlMode,
    erase_mode: EraseMode,
    /// In-progress stroke points, in image-pixel coordinates (empty = idle).
    stroke: Vec<Pos2>,
    /// Undo / redo history of full-image snapshots.
    undo: Vec<RgbaImage>,
    redo: Vec<RgbaImage>,
    /// A short note shown in the status bar (e.g. "Copied to clipboard").
    notice: Option<String>,
}

/// The canvas view transform: how the image is placed inside the canvas rect.
struct View {
    /// Points per image pixel.
    zoom: f32,
    /// Image top-left relative to the canvas rect's min, in points.
    offset: Vec2,
    /// Cleared until the first real canvas size is known, so the opening frame
    /// fits the image to the available area before anything is drawn.
    initialized: bool,
}

impl EditorSession {
    /// Upload the capture and open a session viewing it. `color` seeds the markup
    /// colour (the capture bar's active colour).
    pub fn new(ctx: &egui::Context, image: RgbaImage, color: [u8; 4]) -> Self {
        let texture = upload(ctx, &image);
        let pristine = image.clone();
        Self {
            image,
            pristine,
            texture,
            view: View {
                zoom: 1.0,
                offset: Vec2::ZERO,
                initialized: false,
            },
            tool: Tool::Pan,
            color,
            pen_width: 3.0,
            brush_width: 12.0,
            hl_width: 24.0,
            eraser_width: 16.0,
            hl_mode: HlMode::Free,
            erase_mode: EraseMode::White,
            stroke: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            notice: None,
        }
    }

    /// Consume the session and return the working image (on Save).
    pub fn into_image(self) -> RgbaImage {
        self.image
    }

    /// A copy of the current working image (for Copy-to-clipboard while editing).
    pub fn flatten(&self) -> RgbaImage {
        self.image.clone()
    }

    /// Image size in pixels, as a [`Vec2`].
    fn image_size(&self) -> Vec2 {
        egui::vec2(self.image.width() as f32, self.image.height() as f32)
    }

    /// Draw the editor and process input for one frame.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> EditorOutcome {
        // Keyboard: Ctrl/Cmd+Z undo, Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z redo.
        let (undo_key, redo_key) = ui.input(|i| {
            let cmd = i.modifiers.command;
            let undo = cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z);
            let redo = cmd
                && (i.key_pressed(egui::Key::Y)
                    || (i.modifiers.shift && i.key_pressed(egui::Key::Z)));
            (undo, redo)
        });
        if undo_key {
            self.undo();
        }
        if redo_key {
            self.redo();
        }

        // Esc: cancel an in-progress stroke; if nothing has been drawn yet, it
        // discards the capture (the Phase 3 behaviour). Once edits exist, Esc is
        // ignored so a stray press can't throw the work away — use Discard.
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if !self.stroke.is_empty() {
                self.stroke.clear();
            } else if self.undo.is_empty() {
                return EditorOutcome::Discard;
            }
        }

        let mut outcome = EditorOutcome::Active;
        egui::Panel::top("freally_toolbar2")
            .resizable(false)
            .show_inside(ui, |ui| self.tool_strip(ui));
        egui::Panel::bottom("freally_editor_actions")
            .resizable(false)
            .show_inside(ui, |ui| {
                if let Some(o) = self.action_bar(ui) {
                    outcome = o;
                }
            });
        egui::CentralPanel::default().show_inside(ui, |ui| self.canvas(ui));
        outcome
    }

    /// **Toolbar 2** — the markup tool strip plus the active tool's options.
    /// Pen / Brush / Highlighter / Eraser are live (P4.2); the rest are present
    /// but disabled, each labelled with the prompt that enables it.
    fn tool_strip(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            self.tool_button(
                ui,
                Tool::Pan,
                "Pan",
                "Move the view — drag to pan, scroll to zoom",
            );
            ui.separator();
            self.tool_button(ui, Tool::Pen, "Pen", "Freehand pen");
            self.tool_button(ui, Tool::Brush, "Brush", "Thicker brush");
            self.tool_button(
                ui,
                Tool::Highlighter,
                "Highlighter",
                "Translucent highlighter — free or text-aware",
            );
            self.tool_button(
                ui,
                Tool::Eraser,
                "Eraser",
                "Eraser — erase to white, or remove markup only",
            );
            ui.separator();
            disabled_tool(ui, "Text", "Text object — arrives in Phase 4 (P4.3)");
            disabled_tool(
                ui,
                "Shapes",
                "Rectangle / oval / line / arrow — Phase 4 (P4.3)",
            );
            disabled_tool(
                ui,
                "Watermark",
                "Watermark object — arrives in Phase 4 (P4.3)",
            );
            disabled_tool(
                ui,
                "Emoji",
                "Emoji picker (colour) — arrives in Phase 4 (P4.7)",
            );
            disabled_tool(ui, "Image", "Place an image — arrives in Phase 4 (P4.8)");
            ui.separator();
            disabled_tool(
                ui,
                "Filters ▾",
                "Grayscale / blur / cartoonize / … — Phase 4 (P4.5)",
            );
            disabled_tool(
                ui,
                "Transform",
                "Rotate / crop / bevel — arrives in Phase 4 (P4.6)",
            );
            disabled_tool(
                ui,
                "Eyedropper",
                "Pick a colour — arrives in Phase 4 (P4.6)",
            );
            disabled_tool(
                ui,
                "Extract Text",
                "OCR the image to the clipboard — arrives in Phase 4 (P4.6)",
            );
        });
        ui.add_space(2.0);
        ui.separator();
        self.tool_options(ui);
        ui.add_space(2.0);
    }

    /// A selectable button for a live tool.
    fn tool_button(&mut self, ui: &mut egui::Ui, tool: Tool, label: &str, hover: &str) {
        if ui
            .selectable_label(self.tool == tool, label)
            .on_hover_text(hover)
            .clicked()
        {
            self.tool = tool;
        }
    }

    /// Options for the active drawing tool: size (per-tool), colour, and the
    /// highlighter / eraser mode toggles.
    fn tool_options(&mut self, ui: &mut egui::Ui) {
        if !self.tool.is_drawing() {
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Pick a tool above to draw · drag to pan · scroll to zoom")
                    .weak(),
            );
            return;
        }
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            // Size — per-tool, so each tool keeps its own thickness (P4.2).
            ui.label("Size");
            let mut width = self.width();
            if ui
                .add(egui::Slider::new(&mut width, MIN_WIDTH..=MAX_WIDTH).suffix(" px"))
                .on_hover_text("Stroke size in image pixels")
                .changed()
            {
                self.set_width(width);
            }

            // Colour — pen / brush / highlighter (the eraser has no colour).
            if !matches!(self.tool, Tool::Eraser) {
                ui.separator();
                ui.label("Color");
                ui.color_edit_button_srgba_unmultiplied(&mut self.color)
                    .on_hover_text("Markup colour");
            }

            // Mode toggles.
            match self.tool {
                Tool::Highlighter => {
                    ui.separator();
                    ui.selectable_value(&mut self.hl_mode, HlMode::Free, "Free")
                        .on_hover_text("Highlight anything under the stroke");
                    ui.selectable_value(&mut self.hl_mode, HlMode::TextAware, "Text-aware")
                        .on_hover_text("Highlight only detected text, sparing the background");
                }
                Tool::Eraser => {
                    ui.separator();
                    ui.selectable_value(&mut self.erase_mode, EraseMode::White, "To white")
                        .on_hover_text("Paint white");
                    ui.selectable_value(&mut self.erase_mode, EraseMode::MarkupOnly, "Markup only")
                        .on_hover_text("Restore the original captured pixels");
                }
                _ => {}
            }
        });
    }

    /// The bottom bar: zoom controls + status on the left, Undo/Redo and the file
    /// actions (Copy / Save / Discard) on the right.
    fn action_bar(&mut self, ui: &mut egui::Ui) -> Option<EditorOutcome> {
        let mut outcome = None;
        let (mut want_undo, mut want_redo) = (false, false);
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            // Zoom controls (left). ASCII "-"/"+" — the fullwidth/typographic
            // variants are tofu in egui's default fonts (see the capture bar).
            if ui.button(" - ").on_hover_text("Zoom out").clicked() {
                self.zoom_by(1.0 / 1.25, None);
            }
            ui.label(format!("{:.0}%", self.view.zoom * 100.0));
            if ui.button(" + ").on_hover_text("Zoom in").clicked() {
                self.zoom_by(1.25, None);
            }
            if ui
                .button("Fit")
                .on_hover_text("Fit the image to the window")
                .clicked()
            {
                self.view.initialized = false; // re-fit next frame
            }
            if ui
                .button("100%")
                .on_hover_text("Show the image at actual size")
                .clicked()
            {
                self.zoom_by(1.0 / self.view.zoom, None);
            }

            ui.separator();
            ui.label(
                egui::RichText::new(format!("{} × {}", self.image.width(), self.image.height()))
                    .weak(),
            );
            if let Some(notice) = &self.notice {
                ui.separator();
                ui.label(egui::RichText::new(notice).italics().weak());
            }

            // Undo/Redo + file actions (right).
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 🗑 (U+1F5D1) renders via egui's bundled emoji-icon font, unlike
                // "✕" U+2715, which is tofu in the default fonts (see the capture bar).
                if ui
                    .button("🗑 Discard")
                    .on_hover_text("Throw this capture away")
                    .clicked()
                {
                    outcome = Some(EditorOutcome::Discard);
                }
                if ui
                    .button("Save")
                    .on_hover_text("Save to your folder and copy to the clipboard")
                    .clicked()
                {
                    outcome = Some(EditorOutcome::Save);
                }
                if ui
                    .button("Copy")
                    .on_hover_text("Copy the image to the clipboard")
                    .clicked()
                {
                    self.notice = Some("Copied to clipboard".to_owned());
                    outcome = Some(EditorOutcome::Copy);
                }
                ui.separator();
                if ui
                    .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                    .on_hover_text("Redo (Ctrl+Y)")
                    .clicked()
                {
                    want_redo = true;
                }
                if ui
                    .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                    .on_hover_text("Undo (Ctrl+Z)")
                    .clicked()
                {
                    want_undo = true;
                }
            });
        });
        if want_undo {
            self.undo();
        }
        if want_redo {
            self.redo();
        }
        ui.add_space(2.0);
        outcome
    }

    /// The zoom/pan canvas: a checkerboard backdrop (so transparency shows), the
    /// image, a border, and — with a drawing tool — the live stroke preview and a
    /// brush-size ring at the cursor.
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let canvas = ui.max_rect();
        if canvas.width() <= 0.0 || canvas.height() <= 0.0 {
            return;
        }

        // First frame (or after "Fit"): fit the image to the canvas and center it.
        if !self.view.initialized {
            self.fit(canvas.size());
            self.view.initialized = true;
        }

        let response = ui.interact(
            canvas,
            ui.id().with("freally_editor_canvas"),
            Sense::click_and_drag(),
        );

        // Zoom: plain wheel or pinch, centered on the pointer.
        if response.hovered() {
            let (scroll_y, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let mut factor = pinch;
            if scroll_y != 0.0 {
                factor *= (scroll_y * 0.0015).exp();
            }
            if (factor - 1.0).abs() > f32::EPSILON {
                let pivot = response
                    .hover_pos()
                    .map(|p| p - canvas.min)
                    .unwrap_or_else(|| canvas.size() * 0.5);
                self.zoom_by(factor, Some(pivot));
            }
        }

        // Pan + draw (may bake a stroke and re-upload the texture this frame).
        self.handle_pointer(&response, canvas);

        self.view.offset = clamp_offset(
            self.view.offset,
            self.image_size() * self.view.zoom,
            canvas.size(),
            PAN_KEEP,
        );

        // Paint, clipped to the canvas.
        let painter = ui.painter_at(canvas);
        let image_rect = Rect::from_min_size(
            canvas.min + self.view.offset,
            self.image_size() * self.view.zoom,
        );
        let visible = canvas.intersect(image_rect);
        if visible.is_positive() {
            paint_checkerboard(&painter, visible, image_rect.min);
        }
        painter.image(
            self.texture.id(),
            image_rect,
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.rect_stroke(
            image_rect,
            0.0,
            Stroke::new(1.0, Color32::from_gray(90)),
            StrokeKind::Outside,
        );

        self.draw_stroke_preview(&painter, canvas);
        self.draw_cursor_ring(&painter, canvas, &response);
    }

    /// Handle panning and drawing for one frame. Pan uses the primary button when
    /// the Pan tool is active, otherwise the middle button (so a drawing tool can
    /// still pan). A drawing tool draws with the primary button.
    fn handle_pointer(&mut self, response: &egui::Response, canvas: Rect) {
        let pan_btn = if self.tool.is_drawing() {
            PointerButton::Middle
        } else {
            PointerButton::Primary
        };
        if response.dragged_by(pan_btn) {
            self.view.offset += response.drag_delta();
        }
        if !self.tool.is_drawing() {
            return;
        }

        let pointer = response
            .interact_pointer_pos()
            .or_else(|| response.hover_pos())
            .map(|p| self.screen_to_image(canvas, p));

        if response.drag_started_by(PointerButton::Primary) {
            self.stroke.clear();
            if let Some(p) = pointer {
                self.stroke.push(p);
            }
        } else if response.dragged_by(PointerButton::Primary) {
            if let Some(p) = pointer {
                // Thin the path: keep points ≥ 1 image px apart so a bake over a
                // long stroke stays cheap (matches the freeform lasso's approach).
                if self.stroke.last().is_none_or(|&l| (l - p).length() >= 1.0) {
                    self.stroke.push(p);
                }
            }
        } else if response.drag_stopped_by(PointerButton::Primary) {
            let points = std::mem::take(&mut self.stroke);
            self.commit_stroke(&points);
        } else if response.clicked_by(PointerButton::Primary) && self.stroke.is_empty() {
            // A click without a drag stamps a single dot.
            if let Some(p) = pointer {
                self.commit_stroke(&[p]);
            }
        }
    }

    /// Bake the in-progress `points` (image space) into the raster as one undoable
    /// step, using the active tool's paint mode + size.
    fn commit_stroke(&mut self, points: &[Pos2]) {
        let Some(paint) = self.paint_for_tool() else {
            return;
        };
        if points.is_empty() {
            return;
        }
        self.push_undo();
        let radius = self.width() / 2.0;
        let pts: Vec<(f32, f32)> = points.iter().map(|p| (p.x, p.y)).collect();
        raster::bake_stroke(&mut self.image, &self.pristine, &pts, radius, &paint);
        self.reupload();
        self.stroke.clear();
    }

    /// The active tool's paint mode (`None` for the Pan tool).
    fn paint_for_tool(&self) -> Option<Paint> {
        let rgb = [self.color[0], self.color[1], self.color[2]];
        Some(match self.tool {
            Tool::Pen | Tool::Brush => Paint::Solid(rgb),
            Tool::Highlighter => Paint::Highlight {
                color: rgb,
                alpha: HL_ALPHA,
                text_only: self.hl_mode == HlMode::TextAware,
            },
            Tool::Eraser => match self.erase_mode {
                EraseMode::White => Paint::White,
                EraseMode::MarkupOnly => Paint::Restore,
            },
            Tool::Pan => return None,
        })
    }

    /// The active tool's stroke width (image pixels); 0 for the Pan tool.
    fn width(&self) -> f32 {
        match self.tool {
            Tool::Pen => self.pen_width,
            Tool::Brush => self.brush_width,
            Tool::Highlighter => self.hl_width,
            Tool::Eraser => self.eraser_width,
            Tool::Pan => 0.0,
        }
    }

    /// Set the active tool's stroke width (clamped to the slider range).
    fn set_width(&mut self, width: f32) {
        let width = width.clamp(MIN_WIDTH, MAX_WIDTH);
        match self.tool {
            Tool::Pen => self.pen_width = width,
            Tool::Brush => self.brush_width = width,
            Tool::Highlighter => self.hl_width = width,
            Tool::Eraser => self.eraser_width = width,
            Tool::Pan => {}
        }
    }

    /// Draw the in-progress stroke as a live overlay (committed pixels are already
    /// in the texture). Approximate — the bake on release is the source of truth.
    fn draw_stroke_preview(&self, painter: &egui::Painter, canvas: Rect) {
        if self.stroke.is_empty() {
            return;
        }
        let Some(paint) = self.paint_for_tool() else {
            return;
        };
        let color = preview_color(&paint);
        let width = (self.width() * self.view.zoom).max(1.0);
        let screen: Vec<Pos2> = self
            .stroke
            .iter()
            .map(|&p| self.image_to_screen(canvas, p))
            .collect();
        // Round caps: a dot at each end makes the polyline read as a brush stroke.
        painter.circle_filled(screen[0], width * 0.5, color);
        if screen.len() == 1 {
            return;
        }
        painter.add(egui::Shape::line(screen.clone(), Stroke::new(width, color)));
        if let Some(&last) = screen.last() {
            painter.circle_filled(last, width * 0.5, color);
        }
    }

    /// Draw a ring at the cursor showing the current brush size (drawing tools).
    fn draw_cursor_ring(&self, painter: &egui::Painter, canvas: Rect, response: &egui::Response) {
        if !self.tool.is_drawing() {
            return;
        }
        let Some(p) = response.hover_pos() else {
            return;
        };
        if !canvas.contains(p) {
            return;
        }
        let r = (self.width() * self.view.zoom * 0.5).max(2.0);
        // Two concentric rings (dark over light) read on any background.
        painter.circle_stroke(p, r, Stroke::new(1.5, Color32::from_white_alpha(200)));
        painter.circle_stroke(p, r, Stroke::new(0.75, Color32::from_black_alpha(200)));
    }

    /// Map an image-pixel position to a screen point.
    fn image_to_screen(&self, canvas: Rect, p: Pos2) -> Pos2 {
        canvas.min + self.view.offset + p.to_vec2() * self.view.zoom
    }

    /// Map a screen point to an image-pixel position.
    fn screen_to_image(&self, canvas: Rect, p: Pos2) -> Pos2 {
        ((p - canvas.min - self.view.offset) / self.view.zoom).to_pos2()
    }

    /// Push the current raster onto the undo stack (bounded), clearing redo.
    fn push_undo(&mut self) {
        self.undo.push(self.image.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Undo the last bake.
    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            let current = std::mem::replace(&mut self.image, prev);
            self.redo.push(current);
            self.reupload();
        }
    }

    /// Redo the last undone bake.
    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            let current = std::mem::replace(&mut self.image, next);
            self.undo.push(current);
            self.reupload();
        }
    }

    /// Re-upload the working raster to the GPU texture after it changes.
    fn reupload(&mut self) {
        let size = [self.image.width() as usize, self.image.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, self.image.as_raw());
        self.texture.set(color, egui::TextureOptions::NEAREST);
    }

    /// Fit the image to `avail` (points) and center it.
    fn fit(&mut self, avail: Vec2) {
        self.view.zoom = fit_zoom(self.image_size(), avail);
        self.view.offset = centered_offset(self.image_size() * self.view.zoom, avail);
    }

    /// Multiply the zoom by `factor`, keeping the image point under `pivot`
    /// (canvas-local points; `None` keeps the image centered on the canvas-origin)
    /// fixed on screen. Clamped to [`MIN_ZOOM`, `MAX_ZOOM`].
    fn zoom_by(&mut self, factor: f32, pivot: Option<Vec2>) {
        let old = self.view.zoom;
        let new = (old * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if new == old {
            return;
        }
        let pivot = pivot.unwrap_or(Vec2::ZERO);
        self.view.offset = zoom_about(self.view.offset, pivot, old, new);
        self.view.zoom = new;
    }
}

/// Upload an RGBA image as an egui texture (nearest-neighbour, so zoomed-in pixels
/// stay crisp for pixel-level editing).
fn upload(ctx: &egui::Context, image: &RgbaImage) -> egui::TextureHandle {
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    ctx.load_texture("freally_editor_image", color, egui::TextureOptions::NEAREST)
}

/// A disabled Toolbar 2 tool button (present, but enabled in a later prompt).
fn disabled_tool(ui: &mut egui::Ui, label: &str, arrives: &str) {
    ui.add_enabled(false, egui::Button::new(label))
        .on_disabled_hover_text(arrives);
}

/// Screen colour for the live stroke preview of a given paint mode.
fn preview_color(paint: &Paint) -> Color32 {
    match paint {
        Paint::Solid([r, g, b]) => Color32::from_rgb(*r, *g, *b),
        Paint::Highlight { color, alpha, .. } => {
            Color32::from_rgba_unmultiplied(color[0], color[1], color[2], (alpha * 255.0) as u8)
        }
        // Eraser previews as a neutral swept band (the bake is the real result).
        Paint::White => Color32::from_rgba_unmultiplied(255, 255, 255, 180),
        Paint::Restore => Color32::from_rgba_unmultiplied(128, 128, 128, 140),
    }
}

/// Zoom (points per pixel) that fits an `img`-pixel image within `avail` points.
fn fit_zoom(img: Vec2, avail: Vec2) -> f32 {
    if img.x <= 0.0 || img.y <= 0.0 {
        return 1.0;
    }
    (avail.x / img.x)
        .min(avail.y / img.y)
        .clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Offset that centers an already-scaled image (`scaled` points) within `avail`.
fn centered_offset(scaled: Vec2, avail: Vec2) -> Vec2 {
    (avail - scaled) * 0.5
}

/// New offset after zooming from `old` to `new` while keeping the image point
/// currently under `pivot` (canvas-local points) anchored under the cursor.
fn zoom_about(offset: Vec2, pivot: Vec2, old: f32, new: f32) -> Vec2 {
    // image_pt = (pivot - offset) / old  must stay under `pivot` after zoom:
    // offset' = pivot - image_pt * new
    pivot - (pivot - offset) * (new / old)
}

/// Clamp the pan `offset` so at least `keep` points of the `scaled`-size image
/// stay within an `avail`-size canvas on each axis.
fn clamp_offset(offset: Vec2, scaled: Vec2, avail: Vec2, keep: f32) -> Vec2 {
    let clamp_axis = |off: f32, size: f32, span: f32| {
        let lo = keep - size; // image's right/bottom edge ≥ `keep` from the left/top
        let hi = span - keep; // image's left/top edge ≤ `keep` from the right/bottom
        if lo > hi {
            (lo + hi) * 0.5
        } else {
            off.clamp(lo, hi)
        }
    };
    egui::vec2(
        clamp_axis(offset.x, scaled.x, avail.x),
        clamp_axis(offset.y, scaled.y, avail.y),
    )
}

/// Tile a checkerboard over `area`, with cells aligned to `origin` so the pattern
/// doesn't shimmer while panning. Makes transparent (e.g. freeform-masked) pixels
/// visible behind the image.
fn paint_checkerboard(painter: &egui::Painter, area: Rect, origin: Pos2) {
    const CELL: f32 = 12.0;
    let light = Color32::from_gray(210);
    let dark = Color32::from_gray(170);
    painter.rect_filled(area, 0.0, light);

    // First cell index covering the area, measured from `origin`.
    let start_i = ((area.min.x - origin.x) / CELL).floor() as i64;
    let start_j = ((area.min.y - origin.y) / CELL).floor() as i64;
    let mut j = start_j;
    loop {
        let y0 = origin.y + j as f32 * CELL;
        if y0 >= area.max.y {
            break;
        }
        let mut i = start_i;
        loop {
            let x0 = origin.x + i as f32 * CELL;
            if x0 >= area.max.x {
                break;
            }
            // Paint only the dark squares over the light base.
            if (i + j) & 1 != 0 {
                let cell = Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x0 + CELL, y0 + CELL))
                    .intersect(area);
                if cell.is_positive() {
                    painter.rect_filled(cell, 0.0, dark);
                }
            }
            i += 1;
        }
        j += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_set() {
        assert_eq!(CRATE_NAME, "freally-editor");
    }

    #[test]
    fn fit_zoom_fills_the_smaller_axis_and_clamps() {
        // 200×100 image into a 400×400 box → limited by width (×2).
        assert_eq!(
            fit_zoom(egui::vec2(200.0, 100.0), egui::vec2(400.0, 400.0)),
            2.0
        );
        // 100×200 into 400×400 → limited by height (×2).
        assert_eq!(
            fit_zoom(egui::vec2(100.0, 200.0), egui::vec2(400.0, 400.0)),
            2.0
        );
        // Degenerate sizes fall back to 1.0 and clamp to the zoom range.
        assert_eq!(
            fit_zoom(egui::vec2(0.0, 0.0), egui::vec2(400.0, 400.0)),
            1.0
        );
        assert_eq!(
            fit_zoom(egui::vec2(1.0, 1.0), egui::vec2(9999.0, 9999.0)),
            MAX_ZOOM
        );
    }

    #[test]
    fn centered_offset_centers_the_scaled_image() {
        assert_eq!(
            centered_offset(egui::vec2(100.0, 50.0), egui::vec2(300.0, 250.0)),
            egui::vec2(100.0, 100.0)
        );
    }

    #[test]
    fn zoom_about_keeps_the_pivot_point_fixed() {
        // Pivot at (50, 50); the image point under it must map back to (50, 50).
        let offset = egui::vec2(10.0, 20.0);
        let pivot = egui::vec2(50.0, 50.0);
        let (old, new) = (1.0, 2.0);
        let img_pt = (pivot - offset) / old;
        let off2 = zoom_about(offset, pivot, old, new);
        let back = off2 + img_pt * new; // screen position of that image point
        assert!((back.x - pivot.x).abs() < 1e-3 && (back.y - pivot.y).abs() < 1e-3);
    }

    #[test]
    fn clamp_offset_keeps_image_partly_in_view() {
        // Tiny image, big canvas: can't push it past the keep margin on either side.
        let scaled = egui::vec2(20.0, 20.0);
        let avail = egui::vec2(800.0, 600.0);
        let keep = 40.0;
        // Far off to the left/top is pulled back so ≥ keep stays visible.
        let c = clamp_offset(egui::vec2(-1000.0, -1000.0), scaled, avail, keep);
        assert_eq!(c, egui::vec2(keep - scaled.x, keep - scaled.y));
        // Far off to the right/bottom is pulled back to the opposite limit.
        let c = clamp_offset(egui::vec2(5000.0, 5000.0), scaled, avail, keep);
        assert_eq!(c, egui::vec2(avail.x - keep, avail.y - keep));
    }
}
