//! Tray-Icon über StatusNotifierItem (auf Ubuntu von der vorinstallierten
//! AppIndicator-Erweiterung angezeigt). GTK4 selbst hat keine Tray-API; ksni
//! spricht das D-Bus-Protokoll direkt und läuft in einem eigenen Thread.

use gtk::prelude::*;
use gtk::{gio, glib};
use ksni::blocking::TrayMethods;

use super::autostart;
use crate::config;
use crate::i18n::tr;

struct PortholeTray;

/// Aktiviert eine App-Action vom Tray-Thread aus im GTK-Hauptthread.
fn activate_app_action(name: &'static str) {
    glib::MainContext::default().invoke(move || {
        if let Some(app) = gio::Application::default() {
            app.activate_action(name, None);
        }
    });
}

impl ksni::Tray for PortholeTray {
    fn id(&self) -> String {
        config::APP_ID.into()
    }

    fn title(&self) -> String {
        config::APP_NAME.into()
    }

    fn icon_name(&self) -> String {
        format!("{}-symbolic", config::APP_ID)
    }

    // Ubuntus AppIndicator-Erweiterung öffnet bei Linksklick das Menü und
    // ruft `activate` nur bei Doppelklick; andere Desktops bei einfachem Klick.
    fn activate(&mut self, _x: i32, _y: i32) {
        activate_app_action("capture");
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{CheckmarkItem, StandardItem};

        let item = |label: &str, icon: &str, action: &'static str| -> ksni::MenuItem<Self> {
            StandardItem {
                label: label.into(),
                icon_name: icon.into(),
                activate: Box::new(move |_| activate_app_action(action)),
                ..Default::default()
            }
            .into()
        };

        vec![
            item("Screenshot aufnehmen", "camera-photo-symbolic", "capture"),
            item("Fenster anzeigen", "focus-windows-symbolic", "show"),
            ksni::MenuItem::Separator,
            item(tr("Preferences"), "preferences-system-symbolic", "preferences"),
            CheckmarkItem {
                label: "Beim Anmelden starten".into(),
                checked: autostart::is_enabled(),
                activate: Box::new(|_| {
                    if let Err(err) = autostart::set_enabled(!autostart::is_enabled()) {
                        glib::g_warning!(config::LOG_DOMAIN, "Autostart nicht änderbar: {err}");
                    }
                }),
                ..Default::default()
            }
            .into(),
            item("Beenden", "application-exit-symbolic", "quit"),
        ]
    }
}

/// Startet das Tray im Hintergrund. Fehlt ein Tray-Host (GNOME ohne
/// Erweiterung), bleibt die App über Launcher, Fenster und Tastenkürzel
/// bedienbar.
pub fn start() {
    std::thread::spawn(|| match PortholeTray.spawn() {
        // Das Handle hält den Dienst am Leben – für die Prozesslaufzeit.
        Ok(handle) => std::mem::forget(handle),
        Err(err) => glib::g_message!(config::LOG_DOMAIN, "Kein Tray-Icon: {err}"),
    });
}
