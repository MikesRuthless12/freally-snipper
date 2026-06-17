//! Freally Snipper — application shell.
//!
//! Opens the Windows-11-Snipping-Tool-style home window, manages persisted user
//! settings, and runs the Phase 1 capture flow (hide → snapshot → selection
//! overlay → save). Editing and video features arrive in later phases.
#![forbid(unsafe_code)]
// Release builds are a GUI app: use the Windows subsystem so launching the .exe
// doesn't open a console window (and closing a console can't kill the app). Debug
// builds keep the console so the banner and any logs are visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod delivery;
mod editor;
mod gallery;
mod hotkey;
mod output;
mod overlay;
mod print_screen;
mod settings;
mod tray;

use eframe::egui;

use app::FreallySnipperApp;
use settings::Settings;

/// Brand icon, embedded so the window icon needs no runtime file lookup.
const ICON_PNG: &[u8] = include_bytes!("../assets/Freally_Snipper_Icon_Light.png");

fn main() -> eframe::Result<()> {
    print_banner();

    let settings = Settings::load();
    let icon = load_icon();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Freally Snipper")
        .with_inner_size([900.0, 600.0])
        .with_min_inner_size([640.0, 420.0]);
    if let Some(icon) = icon.clone() {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    // `icon` is reused for the system-tray icon (Windows/macOS).
    eframe::run_native(
        "Freally Snipper",
        native_options,
        Box::new(move |cc| Ok(Box::new(FreallySnipperApp::new(cc, settings, icon)))),
    )
}

/// Print the version banner to stdout (acceptance for build prompt P0.1).
///
/// Uses `writeln!` with the error ignored rather than `println!`: a release build
/// is a Windows GUI app with no console, where `println!` would panic on the
/// failed stdout write. In a terminal or debug build this still prints normally.
fn print_banner() {
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = writeln!(out, "Freally Snipper v{}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "Free, local-first screen capture + image & light video editor."
    );
    let _ = writeln!(out, "(C) 2026 Mike Weaver - All Rights Reserved.");
}

/// Decode the embedded PNG into an egui window icon, trimming transparent margins
/// so the artwork fills the canvas (otherwise Windows renders it smaller than the
/// other taskbar icons). Returns `None` (no icon) rather than failing the launch
/// if decoding ever goes wrong.
fn load_icon() -> Option<egui::IconData> {
    let image = image::load_from_memory(ICON_PNG).ok()?.into_rgba8();
    let image = trim_transparent_to_square(image);
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

/// Crop fully-transparent margins from `image`, then pad the result to a centered
/// square so the artwork fills the icon without distortion. A taskbar icon with a
/// wide transparent border renders visibly smaller than its neighbours; trimming
/// fixes that. A fully-transparent image is returned unchanged.
fn trim_transparent_to_square(image: image::RgbaImage) -> image::RgbaImage {
    let (w, h) = image.dimensions();
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (w, h, 0u32, 0u32);
    let mut any = false;
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[3] > 8 {
            any = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if !any {
        return image;
    }

    let (cw, ch) = (max_x - min_x + 1, max_y - min_y + 1);
    let cropped = image::imageops::crop_imm(&image, min_x, min_y, cw, ch).to_image();
    if cw == ch {
        return cropped;
    }

    // Pad the shorter side equally so the square isn't stretched in the taskbar.
    let side = cw.max(ch);
    let mut square = image::RgbaImage::new(side, side);
    let (ox, oy) = ((side - cw) / 2, (side - ch) / 2);
    image::imageops::overlay(&mut square, &cropped, ox as i64, oy as i64);
    square
}
