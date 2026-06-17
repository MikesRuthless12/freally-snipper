//! System-tray icon so the app can keep running — and keep serving the global
//! hotkey / Print Screen — while its window is "closed" (P1.5 companion).
//!
//! Windows + macOS only: the Linux tray backends need a GTK event loop that
//! conflicts with our winit/eframe backend, so on Linux this is a no-op stub and
//! the setting is disabled (full Linux tray support is deferred to Phase 7).

/// A command surfaced from the tray for the app to act on.
//
// Constructed only on Windows/macOS (the Linux stub never produces one), so the
// variants look dead on Linux — allow it.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum TrayCommand {
    /// Show / restore the home window.
    Open,
    /// Quit the application.
    Quit,
}

#[cfg(any(windows, target_os = "macos"))]
mod imp {
    use super::TrayCommand;
    use eframe::egui;
    use std::sync::mpsc::{Receiver, Sender};
    use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

    pub struct Tray {
        // Kept alive: dropping the `TrayIcon` removes it from the tray.
        tray: TrayIcon,
        commands: Receiver<TrayCommand>,
    }

    impl Tray {
        /// Build the tray icon (from the app's RGBA icon) and start listening for
        /// its menu / click events. `visible` sets the initial visibility.
        pub fn new(
            ctx: &egui::Context,
            rgba: Vec<u8>,
            width: u32,
            height: u32,
            visible: bool,
        ) -> Option<Tray> {
            let icon = Icon::from_rgba(rgba, width, height).ok()?;
            let menu = Menu::new();
            let open = MenuItem::new("Open Freally Snipper", true, None);
            let quit = MenuItem::new("Quit", true, None);
            menu.append(&open).ok()?;
            menu.append(&quit).ok()?;
            let (open_id, quit_id) = (open.id().clone(), quit.id().clone());

            let tray = TrayIconBuilder::new()
                .with_icon(icon)
                .with_tooltip("Freally Snipper")
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .build()
                .ok()?;
            let _ = tray.set_visible(visible);

            let (tx, rx) = std::sync::mpsc::channel();
            spawn_menu_pump(ctx.clone(), tx.clone(), open_id, quit_id);
            spawn_icon_pump(ctx.clone(), tx);
            Some(Tray { tray, commands: rx })
        }

        /// Show or hide the tray icon.
        pub fn set_visible(&self, visible: bool) {
            let _ = self.tray.set_visible(visible);
        }

        /// Drain pending tray events into a single command (Quit wins).
        pub fn poll(&self) -> Option<TrayCommand> {
            let mut open = None;
            for command in self.commands.try_iter() {
                match command {
                    TrayCommand::Quit => return Some(TrayCommand::Quit),
                    TrayCommand::Open => open = Some(TrayCommand::Open),
                }
            }
            open
        }
    }

    /// Forward menu clicks (Open / Quit) to the UI, waking it (the window may be
    /// hidden in the tray).
    fn spawn_menu_pump(
        ctx: egui::Context,
        tx: Sender<TrayCommand>,
        open_id: MenuId,
        quit_id: MenuId,
    ) {
        let spawned = std::thread::Builder::new()
            .name("freally-tray-menu".to_owned())
            .spawn(move || {
                let receiver = MenuEvent::receiver();
                while let Ok(event) = receiver.recv() {
                    let command = if event.id() == &open_id {
                        Some(TrayCommand::Open)
                    } else if event.id() == &quit_id {
                        Some(TrayCommand::Quit)
                    } else {
                        None
                    };
                    if let Some(command) = command {
                        if tx.send(command).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                }
            });
        if let Err(err) = spawned {
            eprintln!("Freally Snipper: could not start tray menu listener: {err}");
        }
    }

    /// Forward a left double-click on the tray icon as "open the window".
    fn spawn_icon_pump(ctx: egui::Context, tx: Sender<TrayCommand>) {
        let spawned = std::thread::Builder::new()
            .name("freally-tray-icon".to_owned())
            .spawn(move || {
                let receiver = TrayIconEvent::receiver();
                while let Ok(event) = receiver.recv() {
                    if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                        if tx.send(TrayCommand::Open).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                }
            });
        if let Err(err) = spawned {
            eprintln!("Freally Snipper: could not start tray icon listener: {err}");
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::TrayCommand;
    use eframe::egui;

    /// Linux stub: no system tray yet (see module docs).
    pub struct Tray;

    impl Tray {
        pub fn new(
            _ctx: &egui::Context,
            _rgba: Vec<u8>,
            _width: u32,
            _height: u32,
            _visible: bool,
        ) -> Option<Tray> {
            None
        }

        pub fn set_visible(&self, _visible: bool) {}

        pub fn poll(&self) -> Option<TrayCommand> {
            None
        }
    }
}

pub use imp::Tray;
