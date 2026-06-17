//! Persisted user settings (JSON in the OS config directory via `directories`).

use std::path::{Path, PathBuf};
use std::time::Duration;

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

/// Optional countdown before a capture starts (P2.1 — Timer ▾). The home window
/// is already hidden while the delay runs, so the user can arrange the screen
/// (open a menu, hover a tooltip, …) before the overlay appears.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimerDelay {
    #[default]
    None,
    Seconds3,
    Seconds5,
    Seconds10,
}

impl TimerDelay {
    pub const ALL: [TimerDelay; 4] = [Self::None, Self::Seconds3, Self::Seconds5, Self::Seconds10];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Off",
            Self::Seconds3 => "3s",
            Self::Seconds5 => "5s",
            Self::Seconds10 => "10s",
        }
    }

    pub fn seconds(self) -> u64 {
        match self {
            Self::None => 0,
            Self::Seconds3 => 3,
            Self::Seconds5 => 5,
            Self::Seconds10 => 10,
        }
    }

    pub fn duration(self) -> Duration {
        Duration::from_secs(self.seconds())
    }
}

/// The Windows `PrintScreenKeyForSnippingEnabled` value as it was *before*
/// Freally Snipper changed it, remembered so disabling the Print-Screen override
/// (P1.5) restores exactly what was there — never a guessed default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrtScPrior {
    /// The value did not exist before we set it (restore = delete it again).
    Absent,
    /// The value existed; restore it to this.
    Value(u32),
}

/// The 18 UI languages — the same set as Freally OS — with English pinned first,
/// then the rest alphabetically by English name. Tuples are
/// `(BCP-47 code, English name, native name)`. The picker (P2.2) persists the
/// choice now; translating the UI through these codes lands in Phase 7 (P7.3).
pub const UI_LANGUAGES: &[(&str, &str, &str)] = &[
    ("en", "English", "English"),
    ("ar", "Arabic", "العربية"),
    ("zh-CN", "Chinese (Simplified)", "简体中文"),
    ("nl", "Dutch", "Nederlands"),
    ("fr", "French", "Français"),
    ("de", "German", "Deutsch"),
    ("hi", "Hindi", "हिन्दी"),
    ("id", "Indonesian", "Bahasa Indonesia"),
    ("it", "Italian", "Italiano"),
    ("ja", "Japanese", "日本語"),
    ("ko", "Korean", "한국어"),
    ("pl", "Polish", "Polski"),
    ("pt-BR", "Portuguese (Brazil)", "Português (Brasil)"),
    ("ru", "Russian", "Русский"),
    ("es", "Spanish", "Español"),
    ("tr", "Turkish", "Türkçe"),
    ("uk", "Ukrainian", "Українська"),
    ("vi", "Vietnamese", "Tiếng Việt"),
];

/// English display name for a UI-language code (falls back to the code itself).
pub fn language_label(code: &str) -> &str {
    UI_LANGUAGES
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, english, _)| *english)
        .unwrap_or(code)
}

/// How many recent captures the home-window gallery remembers (P2.2).
pub const MAX_RECENT: usize = 24;

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
    /// Countdown before a capture starts (Timer ▾).
    pub timer_delay: TimerDelay,
    /// Open the image editor (Toolbar 2) after a capture instead of saving
    /// directly. The editor arrives in Phase 4; until then captures save as now.
    pub show_capture_editor: bool,
    /// Active markup colour (RGBA), set from the toolbar Color picker and reused
    /// by the editor's tools in later phases.
    pub active_color: [u8; 4],
    /// Selected UI language (BCP-47 code from [`UI_LANGUAGES`]); UI translation
    /// itself arrives in Phase 7.
    pub ui_language: String,
    /// Opt-in: register Print Screen to open Freally Snipper (P1.5).
    pub open_with_print_screen: bool,
    /// Keep running in the system tray when the window is closed, so the global
    /// hotkey (incl. Print Screen) still starts captures (Windows/macOS).
    pub minimize_to_tray: bool,
    /// Opt-in: launch at sign-in, minimized to the tray (P4.10). A per-user
    /// autostart entry (not an OS service); reversible.
    pub start_at_login: bool,
    /// Windows-only memory of the prior Print-Screen registry value, so the
    /// override can be cleanly reverted. `None` means we have not changed it.
    pub print_screen_prior: Option<PrtScPrior>,
    /// Recently saved captures, most-recent first (home-window gallery).
    pub recent_captures: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+S".to_owned(),
            save_folder: default_save_folder(),
            default_image_format: ImageFormat::default(),
            theme: Theme::default(),
            default_snippet_mode: SnippetMode::default(),
            timer_delay: TimerDelay::default(),
            show_capture_editor: false,
            active_color: [220, 38, 38, 255],
            ui_language: "en".to_owned(),
            open_with_print_screen: false,
            minimize_to_tray: false,
            start_at_login: false,
            print_screen_prior: None,
            recent_captures: Vec::new(),
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

    /// Record a freshly-saved capture at the front of the recents (most-recent
    /// first), de-duplicated and capped at [`MAX_RECENT`].
    pub fn push_recent(&mut self, path: PathBuf) {
        self.recent_captures.retain(|p| p != &path);
        self.recent_captures.insert(0, path);
        self.recent_captures.truncate(MAX_RECENT);
    }

    /// Drop recents whose files no longer exist (run at startup so the gallery
    /// never shows broken thumbnails for deleted files).
    pub fn prune_recent(&mut self) {
        self.recent_captures.retain(|p| p.exists());
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
            timer_delay: TimerDelay::Seconds5,
            show_capture_editor: true,
            active_color: [1, 2, 3, 4],
            ui_language: "ja".to_owned(),
            open_with_print_screen: true,
            minimize_to_tray: true,
            start_at_login: true,
            print_screen_prior: Some(PrtScPrior::Value(1)),
            recent_captures: vec![PathBuf::from("/tmp/snips/a.png")],
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
        assert_eq!(TimerDelay::ALL.len(), 4);
        assert_eq!(ImageFormat::Png.label(), "PNG");
        assert_eq!(SnippetMode::FullScreen.label(), "Full screen");
        assert_eq!(TimerDelay::None.label(), "Off");
        assert_eq!(TimerDelay::Seconds10.seconds(), 10);
    }

    #[test]
    fn ui_languages_are_18_with_english_first() {
        assert_eq!(UI_LANGUAGES.len(), 18);
        assert_eq!(UI_LANGUAGES[0].0, "en");
        // Every code resolves to its English name; an unknown code echoes back.
        assert_eq!(language_label("ja"), "Japanese");
        assert_eq!(language_label("zz"), "zz");
    }

    #[test]
    fn push_recent_dedupes_caps_and_orders_most_recent_first() {
        let mut s = Settings::default();
        for i in 0..(MAX_RECENT + 5) {
            s.push_recent(PathBuf::from(format!("/tmp/snip-{i}.png")));
        }
        assert_eq!(s.recent_captures.len(), MAX_RECENT);
        // Re-adding an existing path moves it to the front without growing.
        let again = PathBuf::from("/tmp/snip-3.png");
        s.push_recent(again.clone());
        assert_eq!(s.recent_captures.len(), MAX_RECENT);
        assert_eq!(s.recent_captures[0], again);
    }
}
