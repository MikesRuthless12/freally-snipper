//! The capture selection overlay (Phase 1, P1.2/P1.3).
//!
//! Renders a frozen, dimmed snapshot of the whole virtual desktop and lets the
//! user carve out a selection with a crosshair + rubber-band rectangle. Four
//! modes are supported:
//!
//! - **Rectangle** — drag a box.
//! - **Window** — the window under the cursor is highlighted; click to grab its
//!   exact bounds.
//! - **Freeform** — lasso a path; the bounding box is cropped and pixels outside
//!   the path are made transparent.
//! - **Full screen** — the whole desktop (usually handled before the overlay even
//!   opens, but committed here on click/Enter if routed through).
//!
//! Coordinates from `freally-capture` are virtual-desktop pixels. The overlay
//! maps pointer positions to those pixels via the rectangle the snapshot is
//! drawn into, so selection stays correct regardless of the exact window size
//! (uniform-DPI assumption; mixed-DPI multi-monitor is a known later refinement).

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect as ERect, Sense, Stroke, StrokeKind};
use freally_capture::image::RgbaImage;
use freally_capture::{window_at, Composite, Rect as VRect, WindowInfo};

use crate::settings::SnippetMode;

/// Tint multiplier for the dimmed (unselected) snapshot. Gray < 255 darkens.
const DIM: Color32 = Color32::from_gray(96);
/// Full-resolution UV covering the entire snapshot texture.
fn uv_full() -> ERect {
    ERect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
}

/// Outcome of one overlay frame.
pub enum OverlayOutcome {
    /// Still interacting — keep the overlay open.
    Active,
    /// User cancelled (Esc / window close / empty click in some modes).
    Cancelled,
    /// User committed a selection; here is the cropped RGBA capture (no timer).
    Captured(RgbaImage),
    /// User committed a selection but a Timer is set, so the pixels are grabbed
    /// later (after the countdown) — here is just the selection geometry, so the
    /// shot reflects how the screen looks *after* the delay.
    Selected(Selection),
}

/// A committed selection whose pixels are captured after a Timer countdown.
pub enum Selection {
    /// Rectangle / window / full-screen bounds.
    Rect(VRect),
    /// Freeform lasso: the bounding box plus the path to mask outside it.
    Freeform { bbox: VRect, path: Vec<(i32, i32)> },
}

/// Live state for one capture session (one frozen snapshot + selection).
pub struct OverlaySession {
    composite: Composite,
    texture: egui::TextureHandle,
    mode: SnippetMode,
    windows: Vec<WindowInfo>,
    /// Rectangle/Freeform drag anchor, in virtual pixels.
    drag_start: Option<(i32, i32)>,
    /// Freeform lasso points, in virtual pixels.
    lasso: Vec<(i32, i32)>,
    /// When a Timer is set, commit the selection geometry instead of cropping the
    /// frozen snapshot now — the app re-captures the live screen after the
    /// countdown. `false` (Timer Off) keeps the original crop-now behavior.
    defer: bool,
    /// Colour for the Freeform lasso outline — the toolbar's active markup colour.
    outline: Color32,
}

impl OverlaySession {
    /// Upload the snapshot as a GPU texture and start a session in `mode`.
    pub fn new(
        ctx: &egui::Context,
        composite: Composite,
        mode: SnippetMode,
        windows: Vec<WindowInfo>,
        defer: bool,
        outline: Color32,
    ) -> Self {
        let size = [
            composite.image.width() as usize,
            composite.image.height() as usize,
        ];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, composite.image.as_raw());
        let texture = ctx.load_texture("freally_overlay_snapshot", color, Default::default());
        Self {
            composite,
            texture,
            mode,
            windows,
            drag_start: None,
            lasso: Vec::new(),
            defer,
            outline,
        }
    }

    // ---- coordinate mapping (screen points <-> virtual pixels) -------------

    fn comp_size(&self) -> (f32, f32) {
        (
            self.composite.image.width() as f32,
            self.composite.image.height() as f32,
        )
    }

    fn to_virtual(&self, draw: ERect, p: Pos2) -> (i32, i32) {
        let (cw, ch) = self.comp_size();
        let (ox, oy) = self.composite.origin();
        let (dw, dh) = (draw.width(), draw.height());
        if dw <= 0.0 || dh <= 0.0 {
            return (ox, oy); // avoid divide-by-zero NaN on a zero-size window
        }
        let fx = ((p.x - draw.min.x) / dw).clamp(0.0, 1.0);
        let fy = ((p.y - draw.min.y) / dh).clamp(0.0, 1.0);
        (ox + (fx * cw).round() as i32, oy + (fy * ch).round() as i32)
    }

    fn to_screen(&self, draw: ERect, v: (i32, i32)) -> Pos2 {
        let (cw, ch) = self.comp_size();
        let (ox, oy) = self.composite.origin();
        Pos2::new(
            draw.min.x + ((v.0 - ox) as f32 / cw) * draw.width(),
            draw.min.y + ((v.1 - oy) as f32 / ch) * draw.height(),
        )
    }

    fn screen_rect(&self, draw: ERect, r: VRect) -> ERect {
        ERect::from_min_max(
            self.to_screen(draw, (r.x, r.y)),
            self.to_screen(draw, (r.right(), r.bottom())),
        )
    }

    /// UV sub-rect (0..1) of the snapshot texture for a virtual rectangle.
    fn uv_of(&self, r: VRect) -> ERect {
        let (cw, ch) = self.comp_size();
        let (ox, oy) = self.composite.origin();
        ERect::from_min_max(
            Pos2::new((r.x - ox) as f32 / cw, (r.y - oy) as f32 / ch),
            Pos2::new((r.right() - ox) as f32 / cw, (r.bottom() - oy) as f32 / ch),
        )
    }

    /// Draw a virtual rectangle at full brightness (un-dimmed) plus a border.
    fn draw_bright_selection(&self, painter: &egui::Painter, draw: ERect, r: VRect) {
        if r.is_empty() {
            return;
        }
        let screen = self.screen_rect(draw, r);
        painter.image(self.texture.id(), screen, self.uv_of(r), Color32::WHITE);
        painter.rect_stroke(
            screen,
            0.0,
            Stroke::new(2.0, Color32::from_rgb(255, 80, 80)),
            StrokeKind::Inside,
        );
    }

    fn draw_crosshair(&self, painter: &egui::Painter, draw: ERect, p: Pos2) {
        let stroke = Stroke::new(1.0, Color32::from_white_alpha(150));
        painter.line_segment(
            [Pos2::new(draw.min.x, p.y), Pos2::new(draw.max.x, p.y)],
            stroke,
        );
        painter.line_segment(
            [Pos2::new(p.x, draw.min.y), Pos2::new(p.x, draw.max.y)],
            stroke,
        );
    }

    fn draw_hint(&self, painter: &egui::Painter, draw: ERect, text: &str) {
        let pos = Pos2::new(draw.center().x, draw.min.y + 12.0);
        let rect = painter
            .text(
                pos,
                Align2::CENTER_TOP,
                text,
                FontId::proportional(15.0),
                Color32::WHITE,
            )
            .expand(6.0);
        // Draw a translucent backdrop behind the text for legibility, then redraw.
        painter.rect_filled(rect, 4.0, Color32::from_black_alpha(140));
        painter.text(
            pos,
            Align2::CENTER_TOP,
            text,
            FontId::proportional(15.0),
            Color32::WHITE,
        );
    }

    /// Render the overlay and process input for one frame.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> OverlayOutcome {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            return OverlayOutcome::Cancelled;
        }

        let draw = ui.max_rect();
        let painter = ui.painter().clone();

        // Dimmed full snapshot backdrop.
        painter.image(self.texture.id(), draw, uv_full(), DIM);

        let resp = ui.interact(
            draw,
            ui.id().with("overlay_canvas"),
            Sense::click_and_drag(),
        );
        let pointer = resp
            .interact_pointer_pos()
            .or_else(|| resp.hover_pos())
            .or_else(|| ui.input(|i| i.pointer.latest_pos()));
        let pointer_v = pointer.map(|p| self.to_virtual(draw, p));
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));

        let outcome = match self.mode {
            SnippetMode::Rectangle => {
                if resp.drag_started() {
                    self.drag_start = pointer_v;
                }
                if let (Some(a), Some(b)) = (self.drag_start, pointer_v) {
                    let sel = VRect::from_corners(a, b);
                    self.draw_bright_selection(&painter, draw, sel);
                    self.draw_size_label(&painter, draw, sel);
                    if resp.drag_stopped() {
                        self.drag_start = None;
                        return self.commit_rect(sel);
                    }
                } else if let Some(p) = pointer {
                    self.draw_crosshair(&painter, draw, p);
                }
                self.draw_hint(&painter, draw, "Drag to select  ·  Esc to cancel");
                OverlayOutcome::Active
            }
            SnippetMode::Window => {
                // Clamp the window rect to the desktop so the highlight matches
                // the captured crop (maximized windows often sit a few px
                // off-screen). Borrow only — no per-frame clone.
                let hovered = pointer_v
                    .and_then(|(vx, vy)| window_at(&self.windows, vx, vy))
                    .and_then(|w| w.bounds.intersection(&self.composite.bounds));
                if let Some(bounds) = hovered {
                    self.draw_bright_selection(&painter, draw, bounds);
                    self.draw_size_label(&painter, draw, bounds);
                    if resp.clicked() {
                        return self.commit_rect(bounds);
                    }
                }
                self.draw_hint(&painter, draw, "Click a window  ·  Esc to cancel");
                OverlayOutcome::Active
            }
            SnippetMode::Freeform => {
                if resp.drag_started() {
                    self.lasso.clear();
                    if let Some(p) = pointer_v {
                        self.lasso.push(p);
                    }
                }
                if resp.dragged() {
                    if let Some(p) = pointer_v {
                        // Thin the path: keep points a few pixels apart so the
                        // mask's scanline fill stays cheap on release.
                        let far_enough = self
                            .lasso
                            .last()
                            .is_none_or(|&l| (p.0 - l.0).abs() + (p.1 - l.1).abs() >= 3);
                        if far_enough {
                            self.lasso.push(p);
                        }
                    }
                }
                self.draw_lasso(&painter, draw);
                if resp.drag_stopped() {
                    return self.commit_lasso();
                }
                if self.lasso.is_empty() {
                    if let Some(p) = pointer {
                        self.draw_crosshair(&painter, draw, p);
                    }
                }
                self.draw_hint(&painter, draw, "Draw a freeform shape  ·  Esc to cancel");
                OverlayOutcome::Active
            }
            SnippetMode::FullScreen => {
                self.draw_bright_selection(&painter, draw, self.composite.bounds);
                self.draw_hint(
                    &painter,
                    draw,
                    "Full screen  ·  Click or Enter to capture  ·  Esc to cancel",
                );
                if resp.clicked() || enter {
                    return self.commit_rect(self.composite.bounds);
                }
                OverlayOutcome::Active
            }
        };
        outcome
    }

    fn draw_size_label(&self, painter: &egui::Painter, draw: ERect, sel: VRect) {
        if sel.is_empty() {
            return;
        }
        let anchor = self.to_screen(draw, (sel.x, sel.y));
        let text = format!("{} × {}", sel.width, sel.height);
        let pos = Pos2::new(anchor.x, (anchor.y - 22.0).max(draw.min.y + 2.0));
        let rect = painter
            .text(
                pos,
                Align2::LEFT_TOP,
                &text,
                FontId::monospace(13.0),
                Color32::WHITE,
            )
            .expand(3.0);
        painter.rect_filled(rect, 3.0, Color32::from_black_alpha(160));
        painter.text(
            pos,
            Align2::LEFT_TOP,
            &text,
            FontId::monospace(13.0),
            Color32::WHITE,
        );
    }

    fn draw_lasso(&self, painter: &egui::Painter, draw: ERect) {
        let Some((&first_v, rest)) = self.lasso.split_first() else {
            return;
        };
        if rest.is_empty() {
            return;
        }
        // Map points to screen space on the fly (no per-frame Vec allocation).
        // Outline drawn in the toolbar's active markup colour.
        let stroke = Stroke::new(2.0, self.outline);
        let first = self.to_screen(draw, first_v);
        let mut prev = first;
        for &v in rest {
            let cur = self.to_screen(draw, v);
            painter.line_segment([prev, cur], stroke);
            prev = cur;
        }
        // Hint at closure by connecting the last point back to the first.
        painter.line_segment(
            [prev, first],
            Stroke::new(1.0, Color32::from_white_alpha(120)),
        );
    }

    fn commit_rect(&self, r: VRect) -> OverlayOutcome {
        if r.width < 2 || r.height < 2 {
            return OverlayOutcome::Active; // too small — ignore, keep selecting
        }
        self.commit(Selection::Rect(r))
    }

    fn commit_lasso(&mut self) -> OverlayOutcome {
        let path = std::mem::take(&mut self.lasso);
        let Some(bbox) = bounding_rect(&path) else {
            return OverlayOutcome::Active;
        };
        if bbox.width < 2 || bbox.height < 2 {
            return OverlayOutcome::Active;
        }
        self.commit(Selection::Freeform { bbox, path })
    }

    /// Finish a selection: with a Timer, hand back the geometry (the app grabs
    /// live pixels after the countdown); otherwise crop/mask the frozen snapshot
    /// now. Both paths share [`apply_selection`] so the crop+mask logic lives once.
    fn commit(&self, selection: Selection) -> OverlayOutcome {
        if self.defer {
            return OverlayOutcome::Selected(selection);
        }
        match apply_selection(&self.composite, &selection) {
            Some(img) => OverlayOutcome::Captured(img),
            None => OverlayOutcome::Active,
        }
    }
}

/// Apply a committed [`Selection`] to a freshly captured (live) composite,
/// producing the final cropped/masked RGBA. Used for timed captures, where the
/// pixels are grabbed *after* the countdown rather than from the frozen snapshot,
/// so the shot reflects whatever the user arranged during the delay.
pub fn apply_selection(composite: &Composite, selection: &Selection) -> Option<RgbaImage> {
    match selection {
        Selection::Rect(r) => freally_capture::crop_composite(composite, *r).ok(),
        Selection::Freeform { bbox, path } => {
            let mut img = freally_capture::crop_composite(composite, *bbox).ok()?;
            let origin = bbox
                .intersection(&composite.bounds)
                .map_or((bbox.x, bbox.y), |c| (c.x, c.y));
            mask_to_polygon(&mut img, path, origin);
            Some(img)
        }
    }
}

/// Bounding rectangle of a set of virtual-pixel points (inclusive of extents).
fn bounding_rect(points: &[(i32, i32)]) -> Option<VRect> {
    let mut iter = points.iter();
    let &(fx, fy) = iter.next()?;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (fx, fy, fx, fy);
    for &(x, y) in iter {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    Some(VRect::new(
        min_x,
        min_y,
        (max_x - min_x) as u32 + 1,
        (max_y - min_y) as u32 + 1,
    ))
}

/// Make every pixel **outside** the lasso `poly` transparent, in place.
///
/// `origin` is the crop's top-left in virtual coordinates, so polygon point
/// `(vx, vy)` maps to image pixel `(vx - origin.0, vy - origin.1)`. Uses an
/// even-odd **scanline fill** (O(height × edges + pixels)) rather than a
/// per-pixel point-in-polygon test, so masking a large freeform selection on
/// mouse-release stays instant instead of briefly freezing the window.
fn mask_to_polygon(img: &mut RgbaImage, poly: &[(i32, i32)], origin: (i32, i32)) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    if poly.len() < 3 {
        // A degenerate path selects nothing.
        for px in img.pixels_mut() {
            px[3] = 0;
        }
        return;
    }

    // Polygon in image-local float coordinates.
    let local: Vec<(f32, f32)> = poly
        .iter()
        .map(|&(x, y)| ((x - origin.0) as f32, (y - origin.1) as f32))
        .collect();
    let n = local.len();
    let mut crossings: Vec<f32> = Vec::with_capacity(8);

    for y in 0..h {
        let yc = y as f32 + 0.5; // sample at the pixel-row center
        crossings.clear();
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = local[i];
            let (xj, yj) = local[j];
            if (yi > yc) != (yj > yc) {
                crossings.push(xi + (yc - yi) / (yj - yi) * (xj - xi));
            }
            j = i;
        }
        crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Keep pixels whose center falls inside an [c0, c1] span; clear the gaps.
        let mut cursor = 0i32; // first column not yet cleared/kept
        let mut k = 0;
        while k + 1 < crossings.len() {
            let lo = (crossings[k] - 0.5).ceil() as i32;
            let hi = (crossings[k + 1] - 0.5).floor() as i32;
            clear_alpha_span(img, y as u32, cursor, lo);
            cursor = (hi + 1).clamp(cursor, w);
            k += 2;
        }
        clear_alpha_span(img, y as u32, cursor, w);
    }
}

/// Set alpha = 0 for columns `[x0, x1)` of row `y` (clamped to the image).
fn clear_alpha_span(img: &mut RgbaImage, y: u32, x0: i32, x1: i32) {
    let w = img.width() as i32;
    for x in x0.max(0)..x1.min(w) {
        img.get_pixel_mut(x as u32, y)[3] = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freally_capture::image::Rgba;

    #[test]
    fn bounding_rect_covers_all_points() {
        let pts = [(2, 3), (-1, 10), (5, 1)];
        // x: -1..=5 => width 7 ; y: 1..=10 => height 10
        assert_eq!(bounding_rect(&pts), Some(VRect::new(-1, 1, 7, 10)));
        assert_eq!(bounding_rect(&[]), None);
    }

    #[test]
    fn mask_clears_alpha_outside_polygon() {
        // 4x4 opaque image; mask to a triangle covering the top-left.
        let mut img = RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255]));
        let tri = [(0, 0), (4, 0), (0, 4)];
        mask_to_polygon(&mut img, &tri, (0, 0));
        // Inside the triangle stays opaque; the far corner is cleared.
        assert_eq!(img.get_pixel(0, 0)[3], 255);
        assert_eq!(img.get_pixel(3, 3)[3], 0);
        // RGB is preserved where alpha is cleared (only alpha changes).
        assert_eq!(
            [
                img.get_pixel(3, 3)[0],
                img.get_pixel(3, 3)[1],
                img.get_pixel(3, 3)[2]
            ],
            [200, 100, 50]
        );
    }

    #[test]
    fn mask_keeps_a_full_cover_square() {
        // A square covering the whole image keeps every pixel opaque.
        let mut img = RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
        let square = [(0, 0), (4, 0), (4, 4), (0, 4)];
        mask_to_polygon(&mut img, &square, (0, 0));
        assert!(img.pixels().all(|px| px[3] == 255));
    }

    #[test]
    fn mask_clears_border_outside_inset_square() {
        // Keep only the inner square; the border pixels are cleared.
        let mut img = RgbaImage::from_pixel(6, 6, Rgba([1, 2, 3, 255]));
        let square = [(2, 2), (4, 2), (4, 4), (2, 4)];
        mask_to_polygon(&mut img, &square, (0, 0));
        assert_eq!(img.get_pixel(0, 0)[3], 0); // corner cleared
        assert_eq!(img.get_pixel(5, 5)[3], 0); // corner cleared
        assert_eq!(img.get_pixel(3, 3)[3], 255); // inside kept
    }

    #[test]
    fn mask_honors_origin_offset() {
        // Polygon given in virtual coords; origin maps it into the crop.
        let mut img = RgbaImage::from_pixel(4, 4, Rgba([9, 9, 9, 255]));
        // Square at virtual (100,100)-(104,104); crop origin is (100,100).
        let square = [(100, 100), (104, 100), (104, 104), (100, 104)];
        mask_to_polygon(&mut img, &square, (100, 100));
        assert!(img.pixels().all(|px| px[3] == 255));
    }

    #[test]
    fn mask_degenerate_path_clears_everything() {
        let mut img = RgbaImage::from_pixel(3, 3, Rgba([5, 5, 5, 255]));
        mask_to_polygon(&mut img, &[(0, 0), (1, 0)], (0, 0));
        assert!(img.pixels().all(|px| px[3] == 0));
    }
}
