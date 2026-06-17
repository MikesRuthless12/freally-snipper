//! P4.10 — opt-in "Start Freally Snipper when I sign in" (launch-at-login,
//! minimized to the system tray), per-OS.
//!
//! **Deliberately NOT an OS service/daemon:** a Windows Session-0 service, a macOS
//! `LaunchDaemon`, or a Linux system unit has no desktop session, so it could not
//! capture the screen, show the overlay, or receive the global hotkey — screen
//! capture must run in the user's logged-in session. So this registers a
//! per-**user**, session-scoped autostart entry that launches the app with
//! `--minimized`:
//!
//! - **Windows:** `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (safe
//!   `winreg` wrapper, so the crate stays `#![forbid(unsafe_code)]`).
//! - **macOS:** a `LaunchAgent` plist in `~/Library/LaunchAgents/` (`RunAtLoad`).
//! - **Linux:** an XDG autostart entry in `~/.config/autostart/`.
//!
//! The entry is fully reversible (disabling removes it), mirroring the Print
//! Screen takeover. The executable path is resolved with `std::env::current_exe`.

use std::io;

/// macOS LaunchAgent label / Linux desktop-file stem / Windows Run value name.
#[cfg(any(target_os = "macos", all(not(windows), not(target_os = "macos"))))]
const APP_ID: &str = "com.havoc-software.freally-snipper";

/// Enable (write the entry) or disable (remove it) launch-at-login.
pub fn apply(enabled: bool) -> io::Result<()> {
    imp::apply(enabled)
}

#[cfg(windows)]
mod imp {
    use std::io;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "Freally Snipper";

    pub fn apply(enabled: bool) -> io::Result<()> {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
        if enabled {
            let exe = std::env::current_exe()?;
            // Quote the path (spaces) and pass --minimized so it starts in the tray.
            let command = format!("\"{}\" --minimized", exe.display());
            key.set_value(VALUE, &command)?;
        } else if let Err(err) = key.delete_value(VALUE) {
            // "Already gone" is success; anything else propagates.
            if err.kind() != io::ErrorKind::NotFound {
                return Err(err);
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::APP_ID;
    use std::io::{self, Write};
    use std::path::PathBuf;

    pub fn apply(enabled: bool) -> io::Result<()> {
        let path = plist_path()?;
        if enabled {
            let exe = std::env::current_exe()?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let plist = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
                 \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 <plist version=\"1.0\">\n<dict>\n\
                 \t<key>Label</key><string>{APP_ID}</string>\n\
                 \t<key>ProgramArguments</key>\n\
                 \t<array><string>{}</string><string>--minimized</string></array>\n\
                 \t<key>RunAtLoad</key><true/>\n\
                 </dict>\n</plist>\n",
                exe.display()
            );
            let mut file = std::fs::File::create(&path)?;
            file.write_all(plist.as_bytes())?;
        } else {
            remove_if_present(&path)?;
        }
        Ok(())
    }

    fn plist_path() -> io::Result<PathBuf> {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no HOME directory"))?;
        Ok(PathBuf::from(home).join(format!("Library/LaunchAgents/{APP_ID}.plist")))
    }

    fn remove_if_present(path: &std::path::Path) -> io::Result<()> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
mod imp {
    use super::APP_ID;
    use std::io::{self, Write};
    use std::path::PathBuf;

    pub fn apply(enabled: bool) -> io::Result<()> {
        let path = desktop_path()?;
        if enabled {
            let exe = std::env::current_exe()?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let entry = format!(
                "[Desktop Entry]\nType=Application\nName=Freally Snipper\n\
                 Exec=\"{}\" --minimized\nTerminal=false\n\
                 X-GNOME-Autostart-enabled=true\n",
                exe.display()
            );
            let mut file = std::fs::File::create(&path)?;
            file.write_all(entry.as_bytes())?;
        } else {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    fn desktop_path() -> io::Result<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no config directory"))?;
        Ok(base.join(format!("autostart/{APP_ID}.desktop")))
    }
}
