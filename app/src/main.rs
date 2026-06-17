//! Freally Snipper — application shell.
//!
//! Opens the Windows-11-Snipping-Tool-style home window, manages persisted user
//! settings, and runs the Phase 1 capture flow (hide → snapshot → selection
//! overlay → save). Editing and video features arrive in later phases.
#![forbid(unsafe_code)]

mod app;
mod delivery;
mod gallery;
mod hotkey;
mod output;
mod overlay;
mod print_screen;
mod settings;

use eframe::egui;

use app::FreallySnipperApp;
use settings::Settings;

/// Brand icon, embedded so the window icon needs no runtime file lookup.
const ICON_PNG: &[u8] = include_bytes!("../assets/Freally_Snipper_Icon_Dark.png");

fn main() -> eframe::Result<()> {
    print_banner();

    let settings = Settings::load();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Freally Snipper")
        .with_inner_size([900.0, 600.0])
        .with_min_inner_size([640.0, 420.0]);
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Freally Snipper",
        native_options,
        Box::new(move |cc| Ok(Box::new(FreallySnipperApp::new(cc, settings)))),
    )
}

/// Print the version banner to stdout (acceptance for build prompt P0.1).
fn print_banner() {
    let version = env!("CARGO_PKG_VERSION");
    println!("Freally Snipper v{version}");
    println!("Free, local-first screen capture + image & light video editor.");
    println!("(C) 2026 Mike Weaver - All Rights Reserved.");
}

/// Decode the embedded PNG into an egui window icon. Returns `None` (no icon)
/// rather than failing the launch if decoding ever goes wrong.
fn load_icon() -> Option<egui::IconData> {
    let image = image::load_from_memory(ICON_PNG).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}
