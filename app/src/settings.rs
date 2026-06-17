//! Persisted user settings (JSON in the OS config directory via `directories`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    Light,
    #[default]
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ImageFormat {
    #[default]
    Png,
    Jpg,
    Bmp,
    WebP,
}

impl ImageFormat {
    pub const ALL: [ImageFormat; 4] = [Self::Png, Self::Jpg, Self::Bmp, Self::WebP];

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpg => "JPG",
            Self::Bmp => "BMP",
            Self::WebP => "WebP",
        }
    }
}

/// Curated, always-valid capture hotkeys. The UI only lets the user pick from
/// this list (no free-form typing), so a user can't lock themselves out by
/// entering an unparseable shortcut. The first entry is the default.
pub const HOTKEY_PRESETS: &[&str] = &[
    "Ctrl+Shift+S",
    "Ctrl+Shift+A",
    "Ctrl+Shift+X",
    "Alt+Shift+S",
    "F8",
    "F9",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SnippetMode {
    #[default]
    Rectangle,
    Window,
    Freeform,
    FullScreen,
}

impl SnippetMode {
    pub const ALL: [SnippetMode; 4] = [
        Self::Rectangle,
        Self::Window,
        Self::Freeform,
        Self::FullScreen,
    ];

    pub fn label(self) -> &'static str {
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
pub struct Settings {
    /// Global capture hotkey, e.g. "Ctrl+Shift+S" (rebindable later).
    pub hotkey: String,
    /// Where captures are saved.
    pub save_folder: PathBuf,
    /// Default format for saved images.
    pub default_image_format: ImageFormat,
    /// Light or dark UI theme.
    pub theme: Theme,
    /// Default snippet mode armed when a capture starts.
    pub default_snippet_mode: SnippetMode,
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
    pub fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist settings to disk (creating the config directory if needed).
    pub fn save(&self) -> std::io::Result<()> {
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

pub fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("com", "Havoc Software", "Freally Snipper")
}

pub fn settings_path() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.config_dir().join("settings.json"))
}

fn default_save_folder() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|dirs| dirs.picture_dir().map(Path::to_path_buf))
        .map(|pictures| pictures.join("Freally Snipper"))
        .unwrap_or_else(std::env::temp_dir)
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
