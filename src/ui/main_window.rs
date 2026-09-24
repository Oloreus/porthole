use std::cell::RefCell;

use adw::prelude::*;
use gtk::gio;

use crate::config;
use crate::i18n::tr;

thread_local! {
    static WINDOW: RefCell<Option<adw::ApplicationWindow>> = const { RefCell::new(None) };
    static HINT: RefCell<Option<adw::Banner>> = const { RefCell::new(None) };
}

/// Kleines Bedienfenster mit Kamera-Button. Unter Wayland/Mutter kann ein Client
/// sich weder positionieren noch „immer im Vordergrund" setzen – das bleibt dem
/// Nutzer über das Fenstermenü überlassen.
pub fn present(app: &adw::Application) {
    // Nicht über app.windows() suchen: dort stehen auch Previews und Overlays.
    if let Some(window) = WINDOW.with_borrow(Clone::clone) {
        window.present();
        return;
    }

    let capture_button = gtk::Button::builder()
        .icon_name("camera-photo-symbolic")
        .tooltip_text("Screenshot aufnehmen (Alt+S)")
        .action_name("app.capture-from-window")
        .css_classes(["flat"])
        .build();

    let header = adw::HeaderBar::new();
    header.pack_start(&capture_button);

    let menu = gio::Menu::new();
    menu.append(Some(tr("Preferences")), Some("app.preferences"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .primary(true)
        .build();
    header.pack_end(&menu_button);

    let hint = adw::Banner::new("");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&header);
    content.append(&hint);
    HINT.set(Some(hint));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(config::APP_NAME)
        .content(&content)
        .default_width(360)
        .resizable(false)
        // Schließen versteckt nur; die App läuft im Hintergrund weiter.
        .hide_on_close(true)
        .build();
    // Own window group, so its modal dialogs (preferences) only block this
    // window and not the overlay or open previews.
    gtk::WindowGroup::new().add_window(&window);
    window.present();
    WINDOW.set(Some(window));
}

/// The main window, if it has been created (it may be hidden).
pub fn window() -> Option<adw::ApplicationWindow> {
    WINDOW.with_borrow(Clone::clone)
}

/// Holt das Fenster nach vorn und zeigt einen Hinweis, z. B. für die
/// einmalige Screenshot-Freigabe, die GNOME nur der fokussierten App erlaubt.
pub fn present_with_hint(app: &adw::Application, text: &str) {
    present(app);
    set_hint(Some(text));
}

/// Wie `present_with_hint`, zusätzlich mit Button, der `action` auslöst.
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

/// Versteckt das Fenster, damit es nicht im Screenshot landet. `true`, wenn es
/// sichtbar war (dann braucht der Compositor einen Moment zum Ausblenden).
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
