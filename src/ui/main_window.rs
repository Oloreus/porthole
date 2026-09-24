use std::cell::{Cell, RefCell};

use adw::prelude::*;
use gtk::glib;

use super::preferences;
use crate::config;

thread_local! {
    static WINDOW: RefCell<Option<adw::ApplicationWindow>> = const { RefCell::new(None) };
    static HINT: RefCell<Option<adw::Banner>> = const { RefCell::new(None) };
    /// Handler armed by `when_active`.
    static ON_ACTIVE: RefCell<Option<glib::SignalHandlerId>> = const { RefCell::new(None) };
    /// The window only came up to get the screenshot approval.
    static SHOWN_FOR_APPROVAL: Cell<bool> = const { Cell::new(false) };
}

/// Main window = preferences. Rarely seen: it opens from the app launcher or
/// tray, and when GNOME needs the one-time screenshot approval, which it only
/// grants to a focused window – hence the camera button and the hint banner.
pub fn present(app: &adw::Application) {
    // Don't search app.windows(): it also contains previews and overlays.
    if let Some(window) = WINDOW.with_borrow(Clone::clone) {
        window.present();
        return;
    }

    let capture_button = gtk::Button::builder()
        .icon_name("camera-photo-symbolic")
        .tooltip_text("Screenshot aufnehmen (Alt+S)")
        .action_name("app.capture-from-window")
        .build();

    let header = adw::HeaderBar::new();
    header.pack_start(&capture_button);

    let hint = adw::Banner::new("");

    let layout = adw::ToolbarView::new();
    layout.add_top_bar(&header);
    layout.add_top_bar(&hint);
    layout.set_content(Some(&preferences::page()));
    HINT.set(Some(hint));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(config::APP_NAME)
        .content(&layout)
        .default_width(480)
        .default_height(420)
        // Closing only hides; the app keeps running in the background.
        .hide_on_close(true)
        .build();
    // Closed before it got focus: don't fire when opened again later.
    window.connect_hide(|window| {
        disarm_when_active(window);
        SHOWN_FOR_APPROVAL.set(false);
    });
    window.present();
    WINDOW.set(Some(window));
}

/// Brings the window to the front and shows a hint, e.g. for the one-time
/// screenshot approval, which GNOME only grants to the focused app.
pub fn present_with_hint(app: &adw::Application, text: &str) {
    present(app);
    set_hint(Some(text));
}

/// Like `present_with_hint`, for the screenshot approval: if the window
/// wasn't open before, `capture_succeeded` closes it again.
pub fn present_for_approval(app: &adw::Application, text: &str) {
    let was_visible =
        WINDOW.with_borrow(|window| window.as_ref().is_some_and(|window| window.is_visible()));
    present_with_hint(app, text);
    if !was_visible {
        SHOWN_FOR_APPROVAL.set(true);
    }
}

/// A capture went through, so the approval exists: clear the hint and close
/// the window if it only came up for that. `true` if it was closed – it is
/// then still in that capture.
pub fn capture_succeeded() -> bool {
    set_hint(None);
    if !SHOWN_FOR_APPROVAL.replace(false) {
        return false;
    }
    WINDOW.with_borrow(|window| match window {
        Some(window) if window.is_visible() => {
            window.set_visible(false);
            true
        }
        _ => false,
    })
}

/// Like `present_with_hint`, plus a button that triggers `action`.
pub fn present_with_hint_action(app: &adw::Application, text: &str, button: &str, action: &str) {
    present(app);
    show_hint(Some(text), Some((button, action)));
}

pub fn set_hint(text: Option<&str>) {
    show_hint(text, None);
}

fn show_hint(text: Option<&str>, button: Option<(&str, &str)>) {
    HINT.with_borrow(|hint| {
        if let Some(hint) = hint {
            hint.set_title(text.unwrap_or_default());
            hint.set_button_label(button.map(|(label, _)| label));
            hint.set_action_name(button.map(|(_, action)| action));
            hint.set_revealed(text.is_some());
        }
    });
}

pub fn is_active() -> bool {
    WINDOW.with_borrow(|window| window.as_ref().is_some_and(|window| window.is_active()))
}

/// Runs `callback` once, as soon as the window has keyboard focus. Replaces a
/// callback that is still waiting; dropped if the window is hidden first.
pub fn when_active(callback: impl FnOnce() + 'static) {
    let Some(window) = WINDOW.with_borrow(Clone::clone) else {
        return;
    };
    disarm_when_active(&window);
    let callback = Cell::new(Some(callback));
    let handler = window.connect_is_active_notify(move |window| {
        if window.is_active() {
            disarm_when_active(window);
            if let Some(callback) = callback.take() {
                callback();
            }
        }
    });
    ON_ACTIVE.set(Some(handler));
}

fn disarm_when_active(window: &adw::ApplicationWindow) {
    if let Some(handler) = ON_ACTIVE.take() {
        window.disconnect(handler);
    }
}

/// Hides the window so it doesn't end up in the screenshot. Returns `true` if
/// it was visible (the compositor then needs a moment to hide it).
pub fn hide_for_capture() -> bool {
    WINDOW.with_borrow(|window| match window {
        Some(window) if window.is_visible() => {
            window.set_visible(false);
            true
        }
        _ => false,
    })
}

pub fn restore_after_capture() {
    WINDOW.with_borrow(|window| {
        if let Some(window) = window {
            window.set_visible(true);
        }
    });
}
