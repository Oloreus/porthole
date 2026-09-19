use gtk::glib;

use crate::config;

/// Meldet die App-ID beim xdg-desktop-portal an (Host-Registry, XDP >= 1.20).
///
/// Muss der allererste Portal-Aufruf auf ashpds D-Bus-Verbindung sein. Ohne
/// Registrierung ist die App-ID bei Terminal-Start leer: die Screenshot-Permission
/// landet dann global unter "", und BindShortcuts scheitert auf GNOME 48.
/// GTKs eigene Registrierung (4.18+) gilt nur für dessen GDBus-Verbindung.
pub async fn register_host_app() {
    let app_id = match ashpd::AppID::try_from(config::APP_ID) {
        Ok(id) => id,
        Err(err) => {
            glib::g_critical!(config::LOG_DOMAIN, "Ungültige App-ID {}: {err}", config::APP_ID);
            return;
        }
    };

    match ashpd::register_host_app(app_id).await {
        Ok(()) => glib::g_message!(
            config::LOG_DOMAIN,
            "Beim Portal registriert als {}",
            config::APP_ID
        ),
        Err(err) => glib::g_warning!(
            config::LOG_DOMAIN,
            "Portal-Registrierung fehlgeschlagen: {err} – ist {}.desktop installiert? \
             (Entwicklung: build-aux/install-dev.sh)",
            config::APP_ID
        ),
    }
}
