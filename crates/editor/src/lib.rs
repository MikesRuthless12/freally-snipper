//! `freally-editor` — the WYSIWYG image editor (Toolbar 2) for Freally Snipper.
//!
//! Phase 4 lives here. **P4.1** establishes the surface: the captured image on a
//! **zoom/pan canvas**, the **Toolbar 2** tool strip, and working
//! **Undo / Redo / Save / Copy / Discard** actions. The guiding rule is
//! *"Save writes exactly what you see."*
//!
//! The markup *tools* fill the strip in the following prompts and all hang off the
//! same [`EditorSession`]: raster pen/brush/highlighter/eraser (P4.2); movable
//! text / shape / watermark / emoji / image objects (P4.3, P4.7, P4.8); live
//! filters (P4.5); transforms + eyedropper + OCR (P4.6); translate-as-you-type
//! text (P4.9). Until each lands, its button is present but disabled and labelled
//! with the prompt that enables it — the same "present-but-disabled" convention
//! the capture bar uses for not-yet-built modes.
//!
//! The editor is drawn into the app's single OS window (morphed to a decorated
//! editor window), matching the one-window model the capture overlay already uses.

use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use freally_capture::image::RgbaImage;

/// Crate identifier, surfaced in version banners and logs.
pub const CRATE_NAME: &str = "freally-editor";

/// Zoom limits (points per image pixel): from a small overview to pixel-level.
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 16.0;
/// Keep at least this many points of the image inside the canvas when panning, so
/// the picture can never be dragged completely out of view (foolproof controls).
const PAN_KEEP: f32 = 40.0;

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

/// A live editing session over one captured image.
///
/// At P4.1 the working raster is the capture as-is; the markup tools that mutate
/// it (and the undo/redo history that tracks those mutations) arrive in P4.2+.
pub struct EditorSession {
    /// The working raster — exactly what Save writes. Markup bakes here (P4.2+).
    image: RgbaImage,
    /// GPU texture mirroring `image`, re-uploaded whenever the raster changes.
    texture: egui::TextureHandle,
    /// Zoom + pan of the canvas view.
    view: View,
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
    /// Upload the capture and open a session viewing it.
    pub fn new(ctx: &egui::Context, image: RgbaImage) -> Self {
        let texture = upload(ctx, &image);
        Self {
            image,
            texture,
            view: View {
                zoom: 1.0,
                offset: Vec2::ZERO,
                initialized: false,
            },
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
        // Esc discards. There is nothing editable yet at P4.1, so this is the same
        // as the Phase 3 hand-off; once edits exist (P4.2+) this gets a guard.
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            return EditorOutcome::Discard;
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

    /// **Toolbar 2** — the markup tool strip. Every tool is present so the final
    /// layout is visible from P4.1; each is disabled until its prompt lands and
    /// names that prompt in its tooltip (the capture bar's convention for
    /// not-yet-built features). They are enabled, one prompt at a time, in P4.2+.
    fn tool_strip(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            tool(ui, "Pen", "Freehand pen — arrives in Phase 4 (P4.2)");
            tool(ui, "Brush", "Thicker brush — arrives in Phase 4 (P4.2)");
            tool(
                ui,
                "Highlighter",
                "Highlighter (free + text-aware) — Phase 4 (P4.2)",
            );
            tool(
                ui,
                "Eraser",
                "Eraser (to-white / markup-only) — Phase 4 (P4.2)",
            );
            ui.separator();
            tool(ui, "Text", "Text object — arrives in Phase 4 (P4.3)");
            tool(
                ui,
                "Shapes",
                "Rectangle / oval / line / arrow — Phase 4 (P4.3)",
            );
            tool(
                ui,
                "Watermark",
                "Watermark object — arrives in Phase 4 (P4.3)",
            );
            tool(
                ui,
                "Emoji",
                "Emoji picker (colour) — arrives in Phase 4 (P4.7)",
            );
            tool(ui, "Image", "Place an image — arrives in Phase 4 (P4.8)");
            ui.separator();
            tool(
                ui,
                "Filters ▾",
                "Grayscale / blur / cartoonize / … — Phase 4 (P4.5)",
            );
            tool(
                ui,
                "Transform",
                "Rotate / crop / bevel — arrives in Phase 4 (P4.6)",
            );
            tool(
                ui,
                "Eyedropper",
                "Pick a colour — arrives in Phase 4 (P4.6)",
            );
            tool(
                ui,
                "Extract Text",
                "OCR the image to the clipboard — arrives in Phase 4 (P4.6)",
            );
        });
        ui.add_space(2.0);
    }

    /// The bottom bar: zoom controls + status on the left, the file actions
    /// (Copy / Save / Discard) on the right. Returns a terminal outcome if one was
    /// requested this frame.
    fn action_bar(&mut self, ui: &mut egui::Ui) -> Option<EditorOutcome> {
        let mut outcome = None;
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
                // Re-fit on the next frame, once the canvas size is known.
                self.view.initialized = false;
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

            // File actions (right).
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 🗑 (U+1F5D1) renders via egui's bundled emoji-icon font, unlike
                // "✕" U+2715, which is tofu in the default fonts (see the capture bar).
                if ui
                    .button("🗑 Discard")
                    .on_hover_text("Throw this capture away (Esc)")
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
                // Undo / Redo: present from P4.1, enabled once edits exist (P4.2+).
                ui.add_enabled(false, egui::Button::new("Redo"))
                    .on_disabled_hover_text("Nothing to redo (markup tools arrive in P4.2)");
                ui.add_enabled(false, egui::Button::new("Undo"))
                    .on_disabled_hover_text("Nothing to undo (markup tools arrive in P4.2)");
            });
        });
        ui.add_space(2.0);
        outcome
    }

    /// The zoom/pan canvas: a checkerboard backdrop (so transparency shows), the
    /// image, and a thin border. Wheel / pinch zoom around the cursor; primary
    /// or middle drag pans. (Primary-drag pan is temporary — P4.2 hands the
    /// primary button to the active drawing tool and pan moves to the middle
    /// button / a hand tool.)
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

        // Pan: drag with the primary or middle button.
        if response.dragged() {
            self.view.offset += response.drag_delta();
        }
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
fn tool(ui: &mut egui::Ui, label: &str, arrives: &str) {
    ui.add_enabled(false, egui::Button::new(label))
        .on_disabled_hover_text(arrives);
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
