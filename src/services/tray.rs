//! Tray icon via StatusNotifierItem (shown on Ubuntu by the preinstalled
//! AppIndicator extension). GTK4 itself has no tray API; ksni speaks the
//! D-Bus protocol directly and runs on its own thread.

use gtk::prelude::*;
use gtk::{gio, glib};
use ksni::blocking::TrayMethods;

use crate::config;
use crate::i18n::tr;

struct PortholeTray;

/// Activates an app action on the GTK main thread from the tray thread.
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

    // Ubuntu's AppIndicator extension opens the menu on left click and only
    // calls `activate` on double click; other desktops call it on single click.
    fn activate(&mut self, _x: i32, _y: i32) {
        activate_app_action("capture");
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;

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
            item(tr("Preferences"), "preferences-system-symbolic", "show"),
            ksni::MenuItem::Separator,
            item("Beenden", "application-exit-symbolic", "quit"),
        ]
    }
}

/// Starts the tray in the background. Without a tray host (GNOME without
/// the extension), the app stays usable via launcher, window and keyboard
/// shortcut.
pub fn start() {
    std::thread::spawn(|| match PortholeTray.spawn() {
        // The handle keeps the service alive for the lifetime of the process.
        Ok(handle) => std::mem::forget(handle),
        Err(err) => glib::g_message!(config::LOG_DOMAIN, "Kein Tray-Icon: {err}"),
    });
}
