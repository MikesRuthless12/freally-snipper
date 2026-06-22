//! Movable overlay objects (P4.3 + P4.4) — shapes and text that float above the
//! raster, are selected/moved/resized as objects, and are **flattened into the
//! bitmap only on Save**. P4.3 shipped the four shapes (rectangle / oval / line /
//! arrow); P4.4 adds **Text** and **Watermark** (rendered via [`crate::text`]).
//! The same model later carries Emoji (P4.7) and Image (P4.8).
//!
//! Geometry is in **image-pixel** space, so it is resolution-independent and the
//! on-screen preview matches the Save-time bake. For Rect/Oval, `a`/`b` are
//! opposite corners; for Line/Arrow they are the two endpoints; for Text, `a` is
//! the top-left and `b = a + rendered size`.

use std::rc::Rc;

use egui::{Color32, Pos2, Rect, Stroke, StrokeKind, Vec2};
use freally_capture::image::{imageops, RgbaImage};

use crate::raster;
use crate::text::{self, FontFamily};

/// The shape an [`Object`] draws.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    Rect,
    Oval,
    Line,
    Arrow,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 4] = [Self::Rect, Self::Oval, Self::Line, Self::Arrow];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rect => "Rectangle",
            Self::Oval => "Oval",
            Self::Line => "Line",
            Self::Arrow => "Arrow",
        }
    }

    /// Rect/Oval resize by their 4 corners; Line/Arrow by their 2 endpoints.
    fn is_box(self) -> bool {
        matches!(self, Self::Rect | Self::Oval)
    }

    /// Whether the Fill toggle applies (Rect/Oval only).
    pub fn fillable(self) -> bool {
        self.is_box()
    }
}

/// Text object data (P4.4) — for both plain Text and Watermark.
#[derive(Clone)]
pub struct TextData {
    pub string: String,
    /// Font size in image pixels.
    pub font_px: f32,
    pub family: FontFamily,
    /// Last-rendered stamp size in image pixels, kept so `bounds()` is self-contained.
    pub size: (u32, u32),
}

impl TextData {
    /// The text actually rendered/baked.
    pub fn display(&self) -> &str {
        &self.string
    }
}

/// Image object data (P4.8) — a placed image, scaled to the object's bounds.
#[derive(Clone)]
pub struct ImageData {
    /// Stable id for the editor's texture cache (survives undo/redo).
    pub id: u64,
    /// Source pixels, shared (`Rc`) so cloning for undo snapshots is cheap.
    pub source: Rc<RgbaImage>,
}

/// What an [`Object`] is.
#[derive(Clone)]
pub enum Kind {
    Shape(ShapeKind),
    Text(TextData),
    Image(ImageData),
}

/// A movable, selectable, resizable overlay object.
#[derive(Clone)]
pub struct Object {
    pub kind: Kind,
    /// Opposite corners (Rect/Oval), endpoints (Line/Arrow), or top-left + size (Text).
    pub a: Pos2,
    pub b: Pos2,
    pub color: [u8; 4],
    /// Stroke width in image pixels (shapes only).
    pub width: f32,
    /// Fill the interior (Rect/Oval only).
    pub fill: bool,
}

impl Object {
    /// The shape kind, or `None` for a text/image object.
    fn shape_kind(&self) -> Option<ShapeKind> {
        match self.kind {
            Kind::Shape(s) => Some(s),
            _ => None,
        }
    }

    /// The text data, if this is a text object.
    pub fn text(&self) -> Option<&TextData> {
        match &self.kind {
            Kind::Text(t) => Some(t),
            _ => None,
        }
    }

    /// Mutable text data, if this is a text object.
    pub fn text_mut(&mut self) -> Option<&mut TextData> {
        match &mut self.kind {
            Kind::Text(t) => Some(t),
            _ => None,
        }
    }

    /// The image data, if this is an image object.
    pub fn image(&self) -> Option<&ImageData> {
        match &self.kind {
            Kind::Image(d) => Some(d),
            _ => None,
        }
    }

    /// Whether this object resizes as an axis-aligned box (Rect/Oval/Image).
    fn is_box_like(&self) -> bool {
        matches!(
            self.kind,
            Kind::Shape(ShapeKind::Rect | ShapeKind::Oval) | Kind::Image(_)
        )
    }

    /// Axis-aligned bounding rectangle (image space).
    pub fn bounds(&self) -> Rect {
        Rect::from_two_pos(self.a, self.b)
    }

    /// Translate the whole object by `delta` (image space).
    pub fn translate(&mut self, delta: Vec2) {
        self.a += delta;
        self.b += delta;
    }

    /// Selection-handle positions (image space): box corners for Rect/Oval/Image,
    /// the two endpoints for Line/Arrow, none for Text (resize via the size property).
    pub fn handles(&self) -> Vec<Pos2> {
        if self.is_box_like() {
            let r = self.bounds();
            vec![
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
            ]
        } else if matches!(self.kind, Kind::Shape(ShapeKind::Line | ShapeKind::Arrow)) {
            vec![self.a, self.b]
        } else {
            Vec::new() // Text
        }
    }

    /// Drag handle `i` to `to`, resizing a box (Rect/Oval/Image) or moving a
    /// Line/Arrow endpoint.
    pub fn drag_handle(&mut self, i: usize, to: Pos2) {
        if self.is_box_like() {
            let r = self.bounds();
            let (mut left, mut top, mut right, mut bottom) =
                (r.left(), r.top(), r.right(), r.bottom());
            match i {
                0 => {
                    left = to.x;
                    top = to.y;
                }
                1 => {
                    right = to.x;
                    top = to.y;
                }
                2 => {
                    right = to.x;
                    bottom = to.y;
                }
                3 => {
                    left = to.x;
                    bottom = to.y;
                }
                _ => {}
            }
            self.a = Pos2::new(left, top);
            self.b = Pos2::new(right, bottom);
        } else {
            match i {
                0 => self.a = to,
                1 => self.b = to,
                _ => {}
            }
        }
    }

    /// Whether `p` (image space) hits the object, within `tol` image pixels.
    pub fn hit(&self, p: Pos2, tol: f32) -> bool {
        let Some(kind) = self.shape_kind() else {
            // Text: the whole rendered box is the hit area.
            return self.bounds().expand(tol).contains(p);
        };
        let half = self.width * 0.5;
        match kind {
            ShapeKind::Rect | ShapeKind::Oval => {
                let r = self.bounds();
                if self.fill {
                    return r.expand(tol).contains(p);
                }
                let inner = r.expand(-(half + tol));
                r.expand(half + tol).contains(p) && !(inner.is_positive() && inner.contains(p))
            }
            ShapeKind::Line | ShapeKind::Arrow => {
                dist_point_segment(p, self.a, self.b) <= half + tol
            }
        }
    }

    /// The image source resized to the object's bounds, with its opacity applied
    /// (the bake path; the preview tints the GPU texture instead).
    fn scaled_image(&self, d: &ImageData) -> RgbaImage {
        let b = self.bounds();
        let bw = b.width().round().max(1.0) as u32;
        let bh = b.height().round().max(1.0) as u32;
        let mut scaled =
            imageops::resize(d.source.as_ref(), bw, bh, imageops::FilterType::Triangle);
        let op = self.color[3] as u16;
        if op < 255 {
            for px in scaled.pixels_mut() {
                px.0[3] = (px.0[3] as u16 * op / 255) as u8;
            }
        }
        scaled
    }

    /// Bake the object into `image` (Save flattening). Text is re-rendered, images
    /// are scaled + composited, shapes are rasterized.
    pub fn bake_into(&self, image: &mut RgbaImage) {
        let Some(kind) = self.shape_kind() else {
            match &self.kind {
                Kind::Text(t) => {
                    if let Some(stamp) = text::render(t.display(), t.font_px, t.family, self.color)
                    {
                        raster::blit_over(
                            image,
                            &stamp,
                            self.a.x.round() as i32,
                            self.a.y.round() as i32,
                        );
                    }
                }
                Kind::Image(d) => {
                    let stamp = self.scaled_image(d);
                    let b = self.bounds();
                    raster::blit_over(
                        image,
                        &stamp,
                        b.min.x.round() as i32,
                        b.min.y.round() as i32,
                    );
                }
                Kind::Shape(_) => {}
            }
            return;
        };
        let rgb = [self.color[0], self.color[1], self.color[2]];
        let radius = (self.width * 0.5).max(0.5);
        match kind {
            ShapeKind::Rect => {
                let b = self.bounds();
                if self.fill {
                    raster::fill_rect(
                        image,
                        (b.left(), b.top()),
                        (b.right(), b.bottom()),
                        self.color,
                    );
                }
                raster::bake_solid_path(image, &rect_outline(b), radius, rgb);
            }
            ShapeKind::Oval => {
                let b = self.bounds();
                let c = b.center();
                let (rx, ry) = (b.width() * 0.5, b.height() * 0.5);
                if self.fill {
                    raster::fill_ellipse(image, (c.x, c.y), (rx, ry), self.color);
                }
                raster::bake_solid_path(image, &ellipse_outline(c, rx, ry), radius, rgb);
            }
            ShapeKind::Line => {
                raster::bake_solid_path(image, &[tup(self.a), tup(self.b)], radius, rgb);
            }
            ShapeKind::Arrow => {
                raster::bake_solid_path(image, &[tup(self.a), tup(self.b)], radius, rgb);
                // Solid triangular head so it reads as an arrow at any width (not a "V").
                let [h1, h2] = arrow_head(self.a, self.b, self.width);
                raster::fill_triangle(image, tup(h1), tup(self.b), tup(h2), self.color);
            }
        }
    }

    /// Draw the *shape* on-screen (live preview). Text objects draw nothing here —
    /// they are drawn by the editor from a cached texture (it owns the egui context).
    pub fn draw(&self, painter: &egui::Painter, to_screen: &impl Fn(Pos2) -> Pos2, scale: f32) {
        let Some(kind) = self.shape_kind() else {
            return;
        };
        let color = self.color32();
        let stroke = Stroke::new((self.width * scale).max(1.0), color);
        match kind {
            ShapeKind::Rect => {
                let r = Rect::from_two_pos(to_screen(self.a), to_screen(self.b));
                if self.fill {
                    painter.rect_filled(r, 0.0, color);
                }
                painter.rect_stroke(r, 0.0, stroke, StrokeKind::Middle);
            }
            ShapeKind::Oval => {
                let r = Rect::from_two_pos(to_screen(self.a), to_screen(self.b));
                let radius = r.size() * 0.5;
                if self.fill {
                    painter.add(egui::Shape::ellipse_filled(r.center(), radius, color));
                }
                painter.add(egui::Shape::ellipse_stroke(r.center(), radius, stroke));
            }
            ShapeKind::Line => {
                painter.line_segment([to_screen(self.a), to_screen(self.b)], stroke);
            }
            ShapeKind::Arrow => {
                let b = to_screen(self.b);
                painter.line_segment([to_screen(self.a), b], stroke);
                // Solid triangular head (matches the bake) so it never looks like a "V".
                let [h1, h2] = arrow_head(self.a, self.b, self.width);
                painter.add(egui::Shape::convex_polygon(
                    vec![to_screen(h1), b, to_screen(h2)],
                    color,
                    Stroke::NONE,
                ));
            }
        }
    }

    fn color32(&self) -> Color32 {
        Color32::from_rgba_unmultiplied(self.color[0], self.color[1], self.color[2], self.color[3])
    }
}

/// Draw the selection chrome (border + handles) for the object at `obj`.
pub fn draw_selection(
    painter: &egui::Painter,
    obj: &Object,
    to_screen: &impl Fn(Pos2) -> Pos2,
    handle_px: f32,
) {
    let accent = Color32::from_rgb(40, 140, 255);
    let r = Rect::from_two_pos(to_screen(obj.a), to_screen(obj.b)).expand(2.0);
    painter.rect_stroke(r, 0.0, Stroke::new(1.0, accent), StrokeKind::Outside);
    for h in obj.handles() {
        let rect = Rect::from_center_size(to_screen(h), Vec2::splat(handle_px));
        painter.rect_filled(rect, 1.0, Color32::WHITE);
        painter.rect_stroke(rect, 1.0, Stroke::new(1.0, accent), StrokeKind::Middle);
    }
}

/// `(f32, f32)` tuple of a [`Pos2`], for the raster bakers.
fn tup(p: Pos2) -> (f32, f32) {
    (p.x, p.y)
}

/// The closed outline of a rectangle as a polyline.
fn rect_outline(r: Rect) -> Vec<(f32, f32)> {
    vec![
        tup(r.left_top()),
        tup(r.right_top()),
        tup(r.right_bottom()),
        tup(r.left_bottom()),
        tup(r.left_top()),
    ]
}

/// The closed outline of an ellipse as a polyline (enough segments to look round).
fn ellipse_outline(center: Pos2, rx: f32, ry: f32) -> Vec<(f32, f32)> {
    const N: usize = 64;
    let mut pts = Vec::with_capacity(N + 1);
    for i in 0..=N {
        let t = i as f32 / N as f32 * std::f32::consts::TAU;
        pts.push((center.x + rx * t.cos(), center.y + ry * t.sin()));
    }
    pts
}

/// The two barb tips of an arrowhead at `b`, pointing back along `a`→`b`.
fn arrow_head(a: Pos2, b: Pos2, width: f32) -> [Pos2; 2] {
    let dir = b - a;
    let len = dir.length();
    if len < 1e-3 {
        return [b, b];
    }
    let unit = dir / len;
    // Head length scales with the line: ~30%, at least 2× the stroke width, capped,
    // and never longer than the line itself.
    let cap = 40.0_f32.max(width * 2.0);
    let head = (len * 0.3).clamp(width * 2.0, cap).min(len);
    let angle = 0.5_f32; // ~28° barbs
    let (sin, cos) = angle.sin_cos();
    let back = -unit;
    let rot = |v: Vec2, s: f32| Vec2::new(v.x * cos - v.y * s, v.x * s + v.y * cos);
    [b + rot(back, sin) * head, b + rot(back, -sin) * head]
}

/// Distance from `p` to segment `a`–`b` (to the point itself if `a == b`).
fn dist_point_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 <= f32::EPSILON {
        return p.distance(a);
    }
    let t = (((p - a).dot(ab)) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use freally_capture::image::{Rgba, RgbaImage};

    fn shape(kind: ShapeKind, a: (f32, f32), b: (f32, f32), fill: bool) -> Object {
        Object {
            kind: Kind::Shape(kind),
            a: Pos2::new(a.0, a.1),
            b: Pos2::new(b.0, b.1),
            color: [255, 0, 0, 255],
            width: 3.0,
            fill,
        }
    }

    #[test]
    fn handle_counts_match_shape_and_text() {
        assert_eq!(
            shape(ShapeKind::Rect, (0.0, 0.0), (10.0, 10.0), false)
                .handles()
                .len(),
            4
        );
        assert_eq!(
            shape(ShapeKind::Line, (0.0, 0.0), (10.0, 10.0), false)
                .handles()
                .len(),
            2
        );
        // Text has no resize handles (size via the property).
        let txt = Object {
            kind: Kind::Text(TextData {
                string: "Hi".into(),
                font_px: 32.0,
                family: FontFamily::Sans,
                size: (40, 40),
            }),
            a: Pos2::ZERO,
            b: Pos2::new(40.0, 40.0),
            color: [0, 0, 0, 255],
            width: 0.0,
            fill: false,
        };
        assert!(txt.handles().is_empty());
        assert!(txt.hit(Pos2::new(20.0, 20.0), 2.0)); // inside its box
    }

    #[test]
    fn filled_rect_hits_interior_hollow_hits_only_border() {
        let filled = shape(ShapeKind::Rect, (0.0, 0.0), (20.0, 20.0), true);
        assert!(filled.hit(Pos2::new(10.0, 10.0), 2.0));
        let hollow = shape(ShapeKind::Rect, (0.0, 0.0), (20.0, 20.0), false);
        assert!(!hollow.hit(Pos2::new(10.0, 10.0), 2.0));
        assert!(hollow.hit(Pos2::new(0.0, 10.0), 2.0));
    }

    #[test]
    fn line_hit_tests_distance_to_segment() {
        let line = shape(ShapeKind::Line, (0.0, 0.0), (10.0, 0.0), false);
        assert!(line.hit(Pos2::new(5.0, 1.0), 2.0));
        assert!(!line.hit(Pos2::new(5.0, 8.0), 2.0));
    }

    #[test]
    fn drag_corner_moves_only_that_corner() {
        let mut o = shape(ShapeKind::Rect, (0.0, 0.0), (10.0, 10.0), false);
        o.drag_handle(0, Pos2::new(-5.0, -5.0));
        assert_eq!(
            o.bounds(),
            Rect::from_min_max(Pos2::new(-5.0, -5.0), Pos2::new(10.0, 10.0))
        );
    }

    #[test]
    fn translate_moves_both_points() {
        let mut o = shape(ShapeKind::Line, (1.0, 2.0), (3.0, 4.0), false);
        o.translate(Vec2::new(10.0, 20.0));
        assert_eq!(o.a, Pos2::new(11.0, 22.0));
        assert_eq!(o.b, Pos2::new(13.0, 24.0));
    }

    #[test]
    fn filled_rect_bakes_its_interior() {
        let mut img = RgbaImage::from_pixel(20, 20, Rgba([255, 255, 255, 255]));
        shape(ShapeKind::Rect, (4.0, 4.0), (16.0, 16.0), true).bake_into(&mut img);
        assert_eq!(img.get_pixel(10, 10).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn line_bakes_along_its_path() {
        let mut img = RgbaImage::from_pixel(20, 6, Rgba([255, 255, 255, 255]));
        shape(ShapeKind::Line, (2.0, 3.0), (18.0, 3.0), false).bake_into(&mut img);
        assert_eq!(img.get_pixel(10, 3).0, [255, 0, 0, 255]);
    }

    #[test]
    fn text_bakes_ink_into_the_image() {
        let mut img = RgbaImage::from_pixel(200, 80, Rgba([255, 255, 255, 255]));
        let txt = Object {
            kind: Kind::Text(TextData {
                string: "Hi".into(),
                font_px: 48.0,
                family: FontFamily::Sans,
                size: (0, 0),
            }),
            a: Pos2::new(10.0, 10.0),
            b: Pos2::new(10.0, 10.0),
            color: [255, 0, 0, 255],
            width: 0.0,
            fill: false,
        };
        txt.bake_into(&mut img);
        // Some pixel changed from white (text ink landed).
        assert!(img.pixels().any(|p| p.0 != [255, 255, 255, 255]));
    }

    #[test]
    fn image_object_is_box_like_and_bakes_scaled() {
        let source = Rc::new(RgbaImage::from_pixel(2, 2, Rgba([0, 200, 0, 255])));
        let obj = Object {
            kind: Kind::Image(ImageData { id: 0, source }),
            a: Pos2::new(2.0, 2.0),
            b: Pos2::new(8.0, 8.0),
            color: [255, 255, 255, 255],
            width: 0.0,
            fill: false,
        };
        assert_eq!(obj.handles().len(), 4); // resizable like a box
        assert!(obj.hit(Pos2::new(5.0, 5.0), 1.0)); // whole box hittable
        let mut img = RgbaImage::from_pixel(12, 12, Rgba([255, 255, 255, 255]));
        obj.bake_into(&mut img);
        assert_eq!(img.get_pixel(5, 5).0, [0, 200, 0, 255]); // scaled green baked in
        assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255, 255]); // outside untouched
    }
}
