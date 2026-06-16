//! Freally Snipper — application shell (Phase 0).
//!
//! Opens the Windows-11-Snipping-Tool-style home window, manages persisted user
//! settings, and prints a version banner on launch. Capture, editing, and video
//! features arrive in later phases.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use eframe::egui;
use serde::{Deserialize, Serialize};

/// 256×256 brand icon, embedded so the window icon needs no runtime file lookup.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

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

// ----------------------------------------------------------------------------
// Settings (persisted as JSON in the OS config directory via `directories`).
// ----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
enum Theme {
    Light,
    #[default]
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
enum ImageFormat {
    #[default]
    Png,
    Jpg,
    Bmp,
    WebP,
}

impl ImageFormat {
    const ALL: [ImageFormat; 4] = [Self::Png, Self::Jpg, Self::Bmp, Self::WebP];

    fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpg => "JPG",
            Self::Bmp => "BMP",
            Self::WebP => "WebP",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
enum SnippetMode {
    #[default]
    Rectangle,
    Window,
    Freeform,
    FullScreen,
}

impl SnippetMode {
    const ALL: [SnippetMode; 4] = [
        Self::Rectangle,
        Self::Window,
        Self::Freeform,
        Self::FullScreen,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::Window => "Window",
            Self::Freeform => "Freeform",
            Self::FullScreen => "Full screen",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    /// Global capture hotkey, e.g. "Ctrl+Shift+S" (rebindable later).
    hotkey: String,
    /// Where captures are saved.
    save_folder: PathBuf,
    /// Default format for saved images.
    default_image_format: ImageFormat,
    /// Light or dark UI theme.
    theme: Theme,
    /// Default snippet mode armed when a capture starts.
    default_snippet_mode: SnippetMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+S".to_owned(),
            save_folder: default_save_folder(),
            default_image_format: ImageFormat::default(),
            theme: Theme::default(),
            default_snippet_mode: SnippetMode::default(),
        }
    }
}

impl Settings {
    /// Load settings from disk, falling back to defaults on any error.
    fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist settings to disk (creating the config directory if needed).
    fn save(&self) -> std::io::Result<()> {
        let Some(path) = settings_path() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no OS config directory available",
            ));
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }
}

fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("com", "Havoc Software", "Freally Snipper")
}

fn settings_path() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.config_dir().join("settings.json"))
}

fn default_save_folder() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|dirs| dirs.picture_dir().map(Path::to_path_buf))
        .map(|pictures| pictures.join("Freally Snipper"))
        .unwrap_or_else(std::env::temp_dir)
}

// ----------------------------------------------------------------------------
// Application
// ----------------------------------------------------------------------------

struct FreallySnipperApp {
    settings: Settings,
}

impl FreallySnipperApp {
    fn new(cc: &eframe::CreationContext<'_>, settings: Settings) -> Self {
        apply_theme(&cc.egui_ctx, settings.theme);
        Self { settings }
    }

    fn persist(&self) {
        if let Err(err) = self.settings.save() {
            eprintln!("Freally Snipper: could not save settings: {err}");
        }
    }
}

fn apply_theme(ctx: &egui::Context, theme: Theme) {
    let preference = match theme {
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    };
    ctx.set_theme(preference);
}

impl eframe::App for FreallySnipperApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Set true whenever a setting changes this frame; persisted once at the end.
        let mut dirty = false;

        egui::Panel::top("toolbar").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("Freally Snipper");
                ui.separator();
                // Future toolbar actions (Phase 1+); disabled placeholders for now.
                for (label, hint) in [
                    ("+ New", "Start a capture — arrives in Phase 1"),
                    ("Camera", "Photo capture — arrives in Phase 1"),
                    ("Video", "Screen recording — arrives in Phase 5"),
                ] {
                    ui.add_enabled(false, egui::Button::new(label))
                        .on_disabled_hover_text(hint);
                }

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
            ui.add_space(28.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "Press   {}   to start a snip",
                        self.settings.hotkey
                    ))
                    .size(22.0)
                    .strong(),
                );
                ui.add_space(6.0);
                ui.label("Capture, markup, and recording arrive in the next phases.");
            });

            ui.add_space(24.0);
            ui.separator();
            ui.heading("Settings");
            ui.add_space(6.0);

            egui::Grid::new("settings_grid")
                .num_columns(2)
                .spacing([16.0, 10.0])
                .show(ui, |ui| {
                    ui.label("Capture hotkey");
                    if ui
                        .text_edit_singleline(&mut self.settings.hotkey)
                        .lost_focus()
                    {
                        dirty = true;
                    }
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
                    ui.label(self.settings.save_folder.display().to_string());
                    ui.end_row();
                });

            ui.add_space(10.0);
            let path = settings_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<unavailable>".to_owned());
            ui.small(format!("Settings file: {path}"));
        });

        if dirty {
            self.persist();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_json() {
        let original = Settings {
            hotkey: "PrtSc".to_owned(),
            save_folder: PathBuf::from("/tmp/snips"),
            default_image_format: ImageFormat::WebP,
            theme: Theme::Light,
            default_snippet_mode: SnippetMode::Freeform,
        };
        let json = serde_json::to_string_pretty(&original).expect("serialize");
        let restored: Settings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, restored);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        // `#[serde(default)]` lets older/partial config files keep working.
        let restored: Settings = serde_json::from_str("{}").expect("deserialize empty object");
        assert_eq!(restored, Settings::default());
    }

    #[test]
    fn enum_label_lists_are_complete() {
        assert_eq!(ImageFormat::ALL.len(), 4);
        assert_eq!(SnippetMode::ALL.len(), 4);
        assert_eq!(ImageFormat::Png.label(), "PNG");
        assert_eq!(SnippetMode::FullScreen.label(), "Full screen");
    }
}
