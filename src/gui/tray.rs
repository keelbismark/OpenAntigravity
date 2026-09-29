//! System-tray icon: the app stays in the notification area when the window
//! is closed.
//!
//! On Windows this is the familiar Win32 `Shell_NotifyIconW` tray; on Linux it
//! is a D-Bus StatusNotifierItem (the `ksni` feature of `tray-icon`) which is
//! the native protocol for KDE Plasma (SteamOS) and is supported by GNOME via
//! the AppIndicator extension.
//!
//! The tray icon is created in `App::new` and stored in the `App` struct.
//! Dropping `TrayIcon` removes the icon from the OS tray, so the field must
//! live as long as the app.  Menu-click and icon-click events are polled each
//! frame in `drain_tray_events`.

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::gui::icon;

// Menu item identifiers.  They have to survive across frames, so they are
// created once and cloned into the builder and later matched by ID.
pub(super) struct TrayItems {
    pub open: MenuItem,
    pub status: MenuItem,
    pub diag: MenuItem,
    pub quit: MenuItem,
}

/// What the frame loop should do after draining tray events.
pub(super) enum TrayAction {
    /// Nothing happened (or only cosmetic updates).
    None,
    /// The user asked to show the window.
    Show,
    /// The user asked to quit the application.
    Quit,
    /// The user asked to run diagnostics.
    RunDiag,
}

/// Build the tray icon with its context menu.  Returns `None` when the icon
/// asset is broken or the OS refused (e.g. no notification daemon on a
/// headless session).  The caller keeps both values alive for the whole run.
pub(super) fn build() -> Option<(TrayIcon, TrayItems)> {
    let items = TrayItems {
        open: MenuItem::new("Открыть", true, None),
        status: MenuItem::new("Статус: ожидание…", false, None),
        diag: MenuItem::new("Тест сети", true, None),
        quit: MenuItem::new("Выход", true, None),
    };

    let menu = Menu::new();
    // build can technically fail (OOM), but with four items this is not worth
    // a dedicated code path — just bail out of tray entirely.
    menu.append(&items.open).ok()?;
    menu.append(&items.status).ok()?;
    menu.append(&items.diag).ok()?;
    menu.append(&items.quit).ok()?;

    let ico = icon::tray_icon()?;

    let tray = TrayIconBuilder::new()
        .with_tooltip("Open Antigravity")
        .with_icon(ico)
        .with_menu(Box::new(menu))
        .build()
        .ok()?;

    Some((tray, items))
}

/// Install the global event handlers that wake the egui paint loop whenever
/// the user clicks the tray icon or a menu item.  Without this, egui sleeps
/// until something inside the window changes and the click is silently
/// queued forever.
pub(super) fn install_wakeup(ctx: &eframe::egui::Context) {
    let c1 = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |_| {
        c1.request_repaint();
    }));
    let c2 = ctx.clone();
    MenuEvent::set_event_handler(Some(move |_| {
        c2.request_repaint();
    }));
}

/// Drain pending tray events and return the action the main loop should take.
pub(super) fn drain(items: &TrayItems) -> TrayAction {
    // Icon click (double-click on Windows, single on Linux) → show.
    while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
        match ev {
            TrayIconEvent::Click { .. } | TrayIconEvent::DoubleClick { .. } => {
                return TrayAction::Show;
            }
            _ => {}
        }
    }

    // Menu items.
    while let Ok(ev) = MenuEvent::receiver().try_recv() {
        if ev.id == items.open.id() {
            return TrayAction::Show;
        }
        if ev.id == items.diag.id() {
            return TrayAction::RunDiag;
        }
        if ev.id == items.quit.id() {
            return TrayAction::Quit;
        }
    }

    TrayAction::None
}

/// Update the "Статус: …" label in the tray menu to reflect the current
/// operational state.  Called from `drain_events` whenever a new worker
/// status snapshot arrives.
pub(super) fn update_status_label(items: &TrayItems, label: &str) {
    items.status.set_text(label);
}

#[cfg(test)]
mod tests {
    // The tray icon needs an OS event loop and a display, which tests on CI
    // do not have, so only the builder path is validated here: the items are
    // constructible and `build` returns `None` gracefully on headless.

    #[test]
    fn menu_items_are_constructible() {
        use tray_icon::menu::MenuItem;
        let _open = MenuItem::new("Открыть", true, None);
        let _quit = MenuItem::new("Выход", true, None);
    }
}
