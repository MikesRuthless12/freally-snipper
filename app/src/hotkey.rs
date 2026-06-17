//! Global capture hotkey (Phase 1, P1.4).
//!
//! Registers a system-wide shortcut (default `Ctrl+Shift+S`) that opens the
//! capture overlay from anywhere — even when Freally Snipper isn't focused. A
//! background listener flips a flag and wakes the UI when the hotkey fires, so
//! the app doesn't have to poll while idle.
//!
//! The hotkey is parsed from the settings string via `global-hotkey`'s own
//! grammar (e.g. `Ctrl+Shift+S`, `Alt+PrintScreen`, `F8`). If the OS refuses the
//! registration (or the string is invalid), capture still works from the home
//! toolbar — the hotkey is a convenience, never a hard dependency.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

/// Owns the OS hotkey registration plus a flag set when the hotkey is pressed.
pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    current: Option<HotKey>,
    /// Optional secondary binding for the Print Screen key (P1.5), registered
    /// alongside `current` so either shortcut opens a capture.
    print_screen: Option<HotKey>,
    fired: Arc<AtomicBool>,
}

impl Hotkeys {
    /// Create the manager and spawn a listener that sets the "fired" flag and
    /// repaints the UI whenever a registered hotkey is pressed. Returns `None`
    /// if the OS refuses to create the manager.
    pub fn new(ctx: &egui::Context) -> Option<Self> {
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => manager,
            Err(err) => {
                eprintln!("Freally Snipper: global hotkeys unavailable: {err}");
                return None;
            }
        };
        let fired = Arc::new(AtomicBool::new(false));
        spawn_listener(ctx.clone(), Arc::clone(&fired));
        Some(Self {
            manager,
            current: None,
            print_screen: None,
            fired,
        })
    }

    /// Register `spec` (e.g. `"Ctrl+Shift+S"`), replacing any previous binding.
    /// Returns `true` on success. On failure the **previous** binding is kept
    /// (we only drop it once the new one is registered), so a rejected choice
    /// never leaves the app with no working hotkey.
    pub fn set_hotkey(&mut self, spec: &str) -> bool {
        let hotkey: HotKey = match spec.parse() {
            Ok(hotkey) => hotkey,
            Err(err) => {
                eprintln!("Freally Snipper: cannot parse hotkey {spec:?}: {err}");
                return false;
            }
        };
        if self.current == Some(hotkey) {
            return true; // already the active binding
        }
        if let Err(err) = self.manager.register(hotkey) {
            eprintln!("Freally Snipper: cannot register hotkey {spec:?}: {err}");
            return false; // keep the previous binding intact
        }
        if let Some(old) = self.current.replace(hotkey) {
            let _ = self.manager.unregister(old);
        }
        true
    }

    /// Register (or unregister) **Print Screen** as an additional capture
    /// shortcut for the P1.5 "Open Freally Snipper with Print Screen" option.
    /// This is independent of the primary [`set_hotkey`](Self::set_hotkey)
    /// binding — both fire the same capture. Returns `true` on success; a
    /// failure (e.g. the OS has no Print Screen key, or it is already taken)
    /// leaves the primary hotkey untouched.
    pub fn set_print_screen(&mut self, enabled: bool) -> bool {
        if enabled {
            if self.print_screen.is_some() {
                return true; // already registered
            }
            let hotkey: HotKey = match "PrintScreen".parse() {
                Ok(hotkey) => hotkey,
                Err(err) => {
                    eprintln!("Freally Snipper: cannot parse Print Screen hotkey: {err}");
                    return false;
                }
            };
            if let Err(err) = self.manager.register(hotkey) {
                eprintln!("Freally Snipper: cannot register Print Screen: {err}");
                return false;
            }
            self.print_screen = Some(hotkey);
            true
        } else {
            if let Some(hotkey) = self.print_screen.take() {
                let _ = self.manager.unregister(hotkey);
            }
            true
        }
    }

    /// Returns `true` once per press (clears the flag).
    pub fn take_fired(&self) -> bool {
        self.fired.swap(false, Ordering::SeqCst)
    }
}

fn spawn_listener(ctx: egui::Context, fired: Arc<AtomicBool>) {
    let spawned = std::thread::Builder::new()
        .name("freally-hotkey-listener".to_owned())
        .spawn(move || {
            let receiver = GlobalHotKeyEvent::receiver();
            while let Ok(event) = receiver.recv() {
                if event.state == HotKeyState::Pressed {
                    fired.store(true, Ordering::SeqCst);
                    ctx.request_repaint();
                }
            }
        });
    if let Err(err) = spawned {
        eprintln!("Freally Snipper: could not start hotkey listener: {err}");
    }
}
