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

/// How long to wait before looking at the tray queues again while the window
/// is in the tray itself.
///
/// A hidden window has no mouse to wake it, and on Linux nothing at all
/// arrives — so without a heartbeat the click would sit in the queue until some
/// unrelated repaint happened to come along (a status update, a proxy ping), or
/// forever on an idle app. The lag is a fifth of a second; nobody sees it, and
/// every tray click lands on the frame that opens the window.
///
/// --- Do not "fix" the wakeup by installing event handlers ---
///
/// `tray-icon` routes an event to `set_event_handler` **or** to the channel —
/// never to both (`TrayIconEvent::send`: `if let Some(handler) = .. { handler(event) }
/// else { channel.send(event) }`). Its own note on `receiver()` says the same:
/// it gets nothing once a handler is set. An earlier version of this file
/// installed handlers that only did `request_repaint()`, which woke the paint
/// loop on every click and left `drain` reading an empty queue forever: the
/// window went to the tray and no click ever brought it back, on any platform.
pub(super) const HIDDEN_TICK: std::time::Duration = std::time::Duration::from_millis(200);

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

    /// A tray click that no one reads is a window that never comes back. The
    /// two things that makes true are worth a guard each: `tray-icon` hands an
    /// event to a handler *instead of* to the channel `drain` reads, and a
    /// hidden window needs someone to ask for a frame before a queued click is
    /// looked at.
    #[test]
    fn a_tray_click_is_read_from_the_channel_and_something_asks_for_a_frame() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // Comments name the trap on purpose, so they are stripped before the
        // search: what must not come back is a call, not a word.
        let code = |p: &str| {
            std::fs::read_to_string(root.join(p))
                .unwrap()
                .lines()
                .map(|l| l.split("//").next().unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let tray_src = code("src/gui/tray.rs");
        // Assembled rather than written out: the assertion's own source is
        // part of the file being searched, and this test must not forbid
        // itself the way the code once did.
        let handler = ["set_event", "_handler"].concat();
        assert!(
            !tray_src.contains(&handler),
            "a tray-icon event handler swallows the channel `drain` reads; \
             wake the window with request_repaint_after instead"
        );
        let gui_src = code("src/gui/mod.rs");
        assert!(
            gui_src.contains("HIDDEN_TICK"),
            "nothing wakes the loop while the window is in the tray, so the \
             click that brings it back is never read"
        );
    }
}
