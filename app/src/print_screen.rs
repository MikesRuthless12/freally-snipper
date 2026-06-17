//! P1.5 — opt-in "Open Freally Snipper with Print Screen", per-OS.
//!
//! - **Windows:** with explicit consent, set
//!   `HKCU\Control Panel\Keyboard\PrintScreenKeyForSnippingEnabled = 0` (which
//!   frees Print Screen from the built-in Snipping Tool) and register Print
//!   Screen as a capture shortcut. The prior value is remembered in settings so
//!   turning the option off restores exactly what was there before.
//! - **macOS:** the system screenshot shortcuts (⌘⇧3/4/5) can't be overridden by
//!   an app — guide the user to System Settings (with a deep link).
//! - **Linux:** the desktop environment owns Print Screen — guide the user to
//!   rebind it to launch Freally Snipper.
//!
//! The whole crate stays `#![forbid(unsafe_code)]`; on Windows the registry work
//! goes through the safe `winreg` wrapper.

use crate::settings::Settings;

/// What happened when the Print-Screen override was toggled, surfaced to the UI.
//
// Which variants are constructed is platform-conditional (Windows builds the
// `Declined`/`Failed` consent path; macOS/Linux build `Guidance`), so a variant
// that is unused on one OS is essential on another — allow the per-OS dead code.
#[allow(dead_code)]
pub enum KeyOutcome {
    /// Applied successfully; show this confirmation.
    Applied(String),
    /// The user declined the system change (e.g. answered "No" to consent).
    Declined,
    /// Can't be applied automatically — guide the user, optionally offering to
    /// open a settings page (`deep_link`, opened via `opener`).
    Guidance {
        message: String,
        deep_link: Option<String>,
    },
    /// The change failed; show this error (the setting is left unchanged).
    Failed(String),
}

#[cfg(windows)]
mod imp {
    use super::KeyOutcome;
    use crate::settings::{PrtScPrior, Settings};

    const KEYBOARD_KEY: &str = r"Control Panel\Keyboard";
    const PRTSC_VALUE: &str = "PrintScreenKeyForSnippingEnabled";

    /// On Windows, "enabling" actually changes a registry value, so we confirm
    /// first, then remember the prior value and write `0`. Disabling restores it.
    pub fn apply(enabled: bool, settings: &mut Settings) -> KeyOutcome {
        if enabled {
            if !confirm_enable() {
                return KeyOutcome::Declined;
            }
            match set_disabled(settings) {
                Ok(()) => KeyOutcome::Applied(
                    "Print Screen now opens Freally Snipper. Your previous Windows setting is \
                     saved and will be restored when you turn this off."
                        .to_owned(),
                ),
                Err(err) => {
                    KeyOutcome::Failed(format!("Couldn't update the Windows setting: {err}"))
                }
            }
        } else {
            match restore(settings) {
                Ok(()) => KeyOutcome::Applied("Restored Windows' Print Screen default.".to_owned()),
                Err(err) => {
                    KeyOutcome::Failed(format!("Couldn't restore the Windows setting: {err}"))
                }
            }
        }
    }

    fn confirm_enable() -> bool {
        use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
        let result = MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("Free the Print Screen key?")
            .set_description(
                "This changes a Windows setting (PrintScreenKeyForSnippingEnabled = 0) so Print \
                 Screen opens Freally Snipper instead of the built-in Snipping Tool.\n\nFreally \
                 Snipper saves your current setting and restores it when you turn this off. \
                 Continue?",
            )
            .set_buttons(MessageButtons::YesNo)
            .show();
        result == MessageDialogResult::Yes
    }

    fn set_disabled(settings: &mut Settings) -> std::io::Result<()> {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(KEYBOARD_KEY)?;
        // Read the prior value the first time only, but commit it to settings
        // *after* the write succeeds — a failed write must not leave a phantom
        // "prior" that a later enable would mistake for the genuine original.
        let prior = if settings.print_screen_prior.is_none() {
            let read: std::io::Result<u32> = key.get_value(PRTSC_VALUE);
            Some(match read {
                Ok(value) => PrtScPrior::Value(value),
                Err(_) => PrtScPrior::Absent,
            })
        } else {
            None
        };
        key.set_value(PRTSC_VALUE, &0u32)?;
        if let Some(prior) = prior {
            settings.print_screen_prior = Some(prior);
        }
        Ok(())
    }

    fn restore(settings: &mut Settings) -> std::io::Result<()> {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(KEYBOARD_KEY)?;
        match settings.print_screen_prior {
            Some(PrtScPrior::Value(value)) => key.set_value(PRTSC_VALUE, &value)?,
            Some(PrtScPrior::Absent) => {
                // Remove the value we added. "Already gone" (NotFound) is fine;
                // any other failure must propagate so we don't forget the original.
                if let Err(err) = key.delete_value(PRTSC_VALUE) {
                    if err.kind() != std::io::ErrorKind::NotFound {
                        return Err(err);
                    }
                }
            }
            // We never changed it — nothing to restore.
            None => {}
        }
        settings.print_screen_prior = None;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::KeyOutcome;
    use crate::settings::Settings;

    pub fn apply(enabled: bool, _settings: &mut Settings) -> KeyOutcome {
        if enabled {
            KeyOutcome::Guidance {
                message: "macOS reserves the screenshot shortcuts (⌘⇧3 / ⌘⇧4 / ⌘⇧5) and they \
                          can't be changed by an app. Freally Snipper's own hotkey still works \
                          everywhere. To free the system shortcuts, open System Settings ▸ \
                          Keyboard ▸ Keyboard Shortcuts ▸ Screenshots and turn them off or remap \
                          them."
                    .to_owned(),
                deep_link: Some(
                    "x-apple.systempreferences:com.apple.preference.keyboard?Shortcuts".to_owned(),
                ),
            }
        } else {
            KeyOutcome::Applied("Freally Snipper will keep using its own hotkey.".to_owned())
        }
    }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
mod imp {
    use super::KeyOutcome;
    use crate::settings::Settings;

    pub fn apply(enabled: bool, _settings: &mut Settings) -> KeyOutcome {
        if enabled {
            KeyOutcome::Guidance {
                message: "On Linux your desktop environment owns the Print Screen key. Freally \
                          Snipper's own hotkey works everywhere; to launch it with Print Screen, \
                          open your desktop's Keyboard Shortcuts settings (GNOME: Settings ▸ \
                          Keyboard ▸ View and Customize Shortcuts ▸ Screenshots; KDE: System \
                          Settings ▸ Shortcuts) and bind Print Screen to run `freally-snipper`."
                    .to_owned(),
                deep_link: None,
            }
        } else {
            KeyOutcome::Applied("Freally Snipper will keep using its own hotkey.".to_owned())
        }
    }
}

/// Apply (or revert) the Print-Screen override for the current OS. Returns a
/// [`KeyOutcome`] the UI turns into a status line / guidance panel.
pub fn apply(enabled: bool, settings: &mut Settings) -> KeyOutcome {
    imp::apply(enabled, settings)
}
