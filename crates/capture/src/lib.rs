//! `freally-capture` — multi-monitor screen capture (image) for Freally Snipper.
//!
//! This is the Phase 1 capture core. It wraps [`xcap`] (the only third-party
//! backend, behind this interface) and exposes a small, OS-agnostic surface:
//!
//! - [`capture_all`] — grab every monitor as RGBA, DPI/scale-aware.
//! - [`composite`] / [`crop_composite`] — stitch the monitors into one
//!   virtual-desktop image and crop a region out of it (what the selection
//!   overlay needs: capture once, crop many times with no flicker).
//! - [`capture_rect`] — the one-shot convenience: capture + composite + crop.
//! - [`list_windows`] — enumerate top-level windows, front-to-back, for the
//!   "Window" capture mode's hit-testing.
//!
//! ## Coordinates
//!
//! Everything is in **virtual-desktop pixel coordinates**: the union of all
//! monitors, where the origin can be negative (a monitor placed left of / above
//! the primary). On Windows `xcap` reports monitor position and size in physical
//! pixels that match the captured image dimensions, so compositing is exact. The
//! per-monitor [`Monitor::scale`] is preserved for the overlay's DPI mapping.
#![forbid(unsafe_code)]

use std::fmt;

/// Re-export of the exact `image` crate `xcap` is built against, so downstream
/// crates share one `RgbaImage` type (no duplicate-crate version skew).
pub use xcap::image;

use image::RgbaImage;
use xcap::{Monitor as XcapMonitor, Window as XcapWindow};

/// Crate identifier, surfaced in version banners and logs.
pub const CRATE_NAME: &str = "freally-capture";

/// An axis-aligned rectangle in **virtual-desktop pixel coordinates**.
///
/// The origin (`x`, `y`) may be negative; `width`/`height` are non-negative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Construct a rectangle from a top-left origin and a size.
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The exclusive right edge (`x + width`).
    pub const fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    /// The exclusive bottom edge (`y + height`).
    pub const fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    /// `true` when the rectangle encloses no pixels.
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Number of pixels the rectangle covers (`width * height`, overflow-safe).
    pub const fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Build a normalized rectangle spanning two corner points, in any order.
    /// Used to turn a press point + a drag point into a selection rectangle.
    pub fn from_corners(a: (i32, i32), b: (i32, i32)) -> Self {
        let x0 = a.0.min(b.0);
        let y0 = a.1.min(b.1);
        let x1 = a.0.max(b.0);
        let y1 = a.1.max(b.1);
        // Differences are non-negative by construction.
        Self::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }

    /// `true` when `(x, y)` lies inside the half-open rectangle.
    pub const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// The overlapping rectangle of `self` and `other`, or `None` if disjoint.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        if x1 > x0 && y1 > y0 {
            Some(Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
        } else {
            None
        }
    }
}

/// A captured monitor: its pixels plus where it sits on the virtual desktop.
pub struct Monitor {
    /// Full-resolution RGBA screenshot of this monitor.
    pub image: RgbaImage,
    /// Position and size on the virtual desktop (physical pixels).
    pub bounds: Rect,
    /// DPI scale factor (e.g. `1.0`, `1.5`, `2.0`).
    pub scale: f32,
    /// Human-readable monitor name (best effort; may be empty).
    pub name: String,
    /// Whether this is the OS primary monitor.
    pub is_primary: bool,
}

/// A frozen snapshot of the whole virtual desktop stitched into one image.
pub struct Composite {
    /// The stitched RGBA image (size = [`Composite::bounds`]).
    pub image: RgbaImage,
    /// The region of the virtual desktop this image covers (its top-left is the
    /// pixel at image coordinate `(0, 0)`).
    pub bounds: Rect,
}

impl Composite {
    /// Top-left origin of the composite in virtual-desktop coordinates.
    pub fn origin(&self) -> (i32, i32) {
        (self.bounds.x, self.bounds.y)
    }

    /// Consume the composite and return the stitched image (no copy). Useful for
    /// a full-desktop capture, where the whole composite *is* the result.
    pub fn into_image(self) -> RgbaImage {
        self.image
    }
}

/// A top-level OS window, for the "Window" capture mode's hit-testing.
///
/// [`list_windows`] returns these **front-to-back** (topmost first), so the
/// window under the cursor is the first one whose [`WindowInfo::bounds`] contain
/// the point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    /// Backend window id (opaque; stable only within one enumeration).
    pub id: u32,
    /// Window title (may be empty).
    pub title: String,
    /// Owning application name (may be empty).
    pub app_name: String,
    /// Window rectangle on the virtual desktop (physical pixels).
    pub bounds: Rect,
}

/// Errors surfaced by this crate.
#[derive(Debug)]
pub enum CaptureError {
    /// The capture backend failed (wraps `xcap`'s error text).
    Backend(String),
    /// No monitors could be enumerated/captured.
    NoMonitors,
    /// A requested region was empty (zero width or height).
    EmptyRegion,
    /// A requested region lay entirely outside the captured desktop.
    RegionOutsideDesktop,
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(msg) => write!(f, "screen capture backend error: {msg}"),
            Self::NoMonitors => write!(f, "no monitors available to capture"),
            Self::EmptyRegion => write!(f, "capture region is empty"),
            Self::RegionOutsideDesktop => write!(f, "capture region is outside the desktop"),
        }
    }
}

impl std::error::Error for CaptureError {}

impl From<xcap::XCapError> for CaptureError {
    fn from(err: xcap::XCapError) -> Self {
        Self::Backend(err.to_string())
    }
}

/// Result type for this crate.
pub type Result<T> = std::result::Result<T, CaptureError>;

/// Capture every monitor as a full-resolution RGBA image (DPI/scale-aware).
///
/// A monitor that fails to read individually is logged and skipped; the call
/// only errors if *no* monitor could be captured.
pub fn capture_all() -> Result<Vec<Monitor>> {
    let monitors = XcapMonitor::all()?;
    let mut captured = Vec::with_capacity(monitors.len());
    let mut last_err: Option<CaptureError> = None;

    for monitor in &monitors {
        match capture_one(monitor) {
            Ok(m) => captured.push(m),
            Err(err) => {
                log::warn!("freally-capture: skipping a monitor: {err}");
                last_err = Some(err);
            }
        }
    }

    if captured.is_empty() {
        return Err(last_err.unwrap_or(CaptureError::NoMonitors));
    }
    Ok(captured)
}

/// Capture a single `xcap` monitor into our [`Monitor`].
///
/// Size is taken from the captured image (the source of truth for pixel count),
/// position from the monitor's reported origin — consistent on Windows where
/// both are physical pixels.
fn capture_one(monitor: &XcapMonitor) -> Result<Monitor> {
    let image = monitor.capture_image()?;
    let bounds = Rect::new(monitor.x()?, monitor.y()?, image.width(), image.height());
    let name = monitor
        .friendly_name()
        .or_else(|_| monitor.name())
        .unwrap_or_default();
    Ok(Monitor {
        image,
        bounds,
        scale: monitor.scale_factor().unwrap_or(1.0),
        name,
        is_primary: monitor.is_primary().unwrap_or(false),
    })
}

/// The union of every monitor's bounds — the full virtual-desktop rectangle.
/// Returns `None` for an empty slice.
pub fn virtual_bounds(monitors: &[Monitor]) -> Option<Rect> {
    let mut iter = monitors.iter();
    let first = iter.next()?.bounds;
    let (mut min_x, mut min_y) = (first.x, first.y);
    let (mut max_x, mut max_y) = (first.right(), first.bottom());
    for m in iter {
        min_x = min_x.min(m.bounds.x);
        min_y = min_y.min(m.bounds.y);
        max_x = max_x.max(m.bounds.right());
        max_y = max_y.max(m.bounds.bottom());
    }
    Some(Rect::new(
        min_x,
        min_y,
        (max_x - min_x) as u32,
        (max_y - min_y) as u32,
    ))
}

/// Stitch all monitors into one virtual-desktop image (origin-shifted so the
/// top-left monitor pixel lands at image `(0, 0)`). Returns `None` for an empty
/// slice.
pub fn composite(monitors: &[Monitor]) -> Option<Composite> {
    let bounds = virtual_bounds(monitors)?;
    // Opaque black backdrop fills any gap between non-rectangular arrangements.
    let mut canvas =
        RgbaImage::from_pixel(bounds.width, bounds.height, image::Rgba([0, 0, 0, 255]));
    for m in monitors {
        let ox = (m.bounds.x - bounds.x) as i64;
        let oy = (m.bounds.y - bounds.y) as i64;
        // Straight copy (monitors are opaque); clips to the canvas automatically.
        image::imageops::replace(&mut canvas, &m.image, ox, oy);
    }
    Some(Composite {
        image: canvas,
        bounds,
    })
}

/// Crop a virtual-desktop rectangle out of a [`Composite`].
///
/// The rectangle is clipped to the available pixels; a region that lies entirely
/// outside the composite yields [`CaptureError::RegionOutsideDesktop`], and an
/// empty region yields [`CaptureError::EmptyRegion`].
pub fn crop_composite(comp: &Composite, rect: Rect) -> Result<RgbaImage> {
    if rect.is_empty() {
        return Err(CaptureError::EmptyRegion);
    }
    let clipped = rect
        .intersection(&comp.bounds)
        .ok_or(CaptureError::RegionOutsideDesktop)?;
    let local_x = (clipped.x - comp.bounds.x) as u32;
    let local_y = (clipped.y - comp.bounds.y) as u32;
    let sub =
        image::imageops::crop_imm(&comp.image, local_x, local_y, clipped.width, clipped.height);
    Ok(sub.to_image())
}

/// One-shot: capture every monitor, stitch, and crop the given virtual-desktop
/// rectangle. Prefer [`capture_all`] + [`composite`] + [`crop_composite`] when
/// cropping more than once from the same frozen snapshot (e.g. the overlay).
pub fn capture_rect(rect: Rect) -> Result<RgbaImage> {
    let monitors = capture_all()?;
    let comp = composite(&monitors).ok_or(CaptureError::NoMonitors)?;
    crop_composite(&comp, rect)
}

/// Enumerate visible top-level windows **front-to-back** (topmost first),
/// skipping minimized and zero-area windows. Used by the "Window" capture mode:
/// the window under the cursor is the first whose [`WindowInfo::bounds`] contain
/// the point.
pub fn list_windows() -> Result<Vec<WindowInfo>> {
    let windows = XcapWindow::all()?;
    let mut out = Vec::with_capacity(windows.len());
    for w in &windows {
        if w.is_minimized().unwrap_or(false) {
            continue;
        }
        // Skip any window whose geometry can't be read fully — defaulting a
        // missing position to (0, 0) would put a phantom hit-box at the origin.
        let (Ok(x), Ok(y), Ok(width), Ok(height)) = (w.x(), w.y(), w.width(), w.height()) else {
            continue;
        };
        if width == 0 || height == 0 {
            continue;
        }
        out.push(WindowInfo {
            id: w.id().unwrap_or(0),
            title: w.title().unwrap_or_default(),
            app_name: w.app_name().unwrap_or_default(),
            bounds: Rect::new(x, y, width, height),
        });
    }
    Ok(out)
}

/// The front-most window containing `(x, y)`, given a front-to-back list from
/// [`list_windows`]. A convenience for the overlay's hit-testing.
pub fn window_at(windows: &[WindowInfo], x: i32, y: i32) -> Option<&WindowInfo> {
    windows.iter().find(|w| w.bounds.contains(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(rgba))
    }

    fn monitor_at(x: i32, y: i32, image: RgbaImage, primary: bool) -> Monitor {
        let bounds = Rect::new(x, y, image.width(), image.height());
        Monitor {
            image,
            bounds,
            scale: 1.0,
            name: "test".to_owned(),
            is_primary: primary,
        }
    }

    #[test]
    fn crate_name_is_set() {
        assert_eq!(CRATE_NAME, "freally-capture");
    }

    #[test]
    fn rect_from_corners_normalizes_any_order() {
        let a = Rect::from_corners((30, 40), (10, 10));
        assert_eq!(a, Rect::new(10, 10, 20, 30));
        // Same rectangle regardless of which corner is "first".
        assert_eq!(a, Rect::from_corners((10, 40), (30, 10)));
    }

    #[test]
    fn rect_edges_contains_and_area() {
        let r = Rect::new(-5, -5, 10, 20);
        assert_eq!(r.right(), 5);
        assert_eq!(r.bottom(), 15);
        assert_eq!(r.area(), 200);
        assert!(r.contains(-5, -5)); // inclusive top-left
        assert!(!r.contains(5, 0)); // exclusive right edge
        assert!(!r.contains(0, 15)); // exclusive bottom edge
        assert!(Rect::new(0, 0, 0, 10).is_empty()); // zero width => empty
        assert!(!r.is_empty());
    }

    #[test]
    fn rect_intersection() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 50, 100, 100);
        assert_eq!(a.intersection(&b), Some(Rect::new(50, 50, 50, 50)));
        // Touching edges do not overlap.
        assert_eq!(a.intersection(&Rect::new(100, 0, 10, 10)), None);
        // Fully disjoint.
        assert_eq!(a.intersection(&Rect::new(200, 200, 10, 10)), None);
    }

    #[test]
    fn virtual_bounds_unions_monitors_with_negative_origin() {
        // A 100x100 primary at origin, plus a 100x100 monitor to its upper-left.
        let monitors = vec![
            monitor_at(0, 0, solid(100, 100, [255, 0, 0, 255]), true),
            monitor_at(-100, -20, solid(100, 100, [0, 255, 0, 255]), false),
        ];
        let bounds = virtual_bounds(&monitors).expect("non-empty");
        assert_eq!(bounds, Rect::new(-100, -20, 200, 120));
        assert!(virtual_bounds(&[]).is_none());
    }

    #[test]
    fn composite_places_each_monitor_at_its_offset() {
        let monitors = vec![
            monitor_at(0, 0, solid(2, 2, [255, 0, 0, 255]), true), // red, right half
            monitor_at(-2, 0, solid(2, 2, [0, 255, 0, 255]), false), // green, left half
        ];
        let comp = composite(&monitors).expect("composite");
        assert_eq!(comp.bounds, Rect::new(-2, 0, 4, 2));
        assert_eq!(comp.origin(), (-2, 0));
        // Left two columns are green, right two are red.
        assert_eq!(comp.image.get_pixel(0, 0), &Rgba([0, 255, 0, 255]));
        assert_eq!(comp.image.get_pixel(1, 1), &Rgba([0, 255, 0, 255]));
        assert_eq!(comp.image.get_pixel(2, 0), &Rgba([255, 0, 0, 255]));
        assert_eq!(comp.image.get_pixel(3, 1), &Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn crop_composite_returns_requested_region_in_virtual_coords() {
        let monitors = vec![monitor_at(-2, -2, solid(4, 4, [10, 20, 30, 255]), true)];
        let comp = composite(&monitors).expect("composite");
        // Crop a 2x2 region straddling the negative origin.
        let crop = crop_composite(&comp, Rect::new(-1, -1, 2, 2)).expect("crop");
        assert_eq!((crop.width(), crop.height()), (2, 2));
        assert_eq!(crop.get_pixel(0, 0), &Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn crop_composite_clips_to_available_pixels() {
        let monitors = vec![monitor_at(0, 0, solid(4, 4, [1, 2, 3, 255]), true)];
        let comp = composite(&monitors).expect("composite");
        // Requesting beyond the right/bottom edge clips to what exists.
        let crop = crop_composite(&comp, Rect::new(2, 2, 10, 10)).expect("crop");
        assert_eq!((crop.width(), crop.height()), (2, 2));
    }

    #[test]
    fn crop_composite_rejects_empty_and_outside_regions() {
        let monitors = vec![monitor_at(0, 0, solid(4, 4, [0, 0, 0, 255]), true)];
        let comp = composite(&monitors).expect("composite");
        assert!(matches!(
            crop_composite(&comp, Rect::new(0, 0, 0, 5)),
            Err(CaptureError::EmptyRegion)
        ));
        assert!(matches!(
            crop_composite(&comp, Rect::new(100, 100, 5, 5)),
            Err(CaptureError::RegionOutsideDesktop)
        ));
    }

    #[test]
    fn window_at_picks_front_most() {
        // `list_windows` returns front-to-back, so the first match wins.
        let windows = vec![
            WindowInfo {
                id: 1,
                title: "front".to_owned(),
                app_name: "a".to_owned(),
                bounds: Rect::new(0, 0, 50, 50),
            },
            WindowInfo {
                id: 2,
                title: "back".to_owned(),
                app_name: "b".to_owned(),
                bounds: Rect::new(0, 0, 100, 100),
            },
        ];
        assert_eq!(window_at(&windows, 10, 10).map(|w| w.id), Some(1));
        assert_eq!(window_at(&windows, 75, 75).map(|w| w.id), Some(2));
        assert_eq!(window_at(&windows, 200, 200), None);
    }
}
