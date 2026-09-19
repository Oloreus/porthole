//! Zugriff auf den Permission-Store des Portals. Nötig, weil eine verweigerte
//! Screenshot-Berechtigung dauerhaft gilt und GNOME-Einstellungen sie für
//! Host-Apps (kein Flatpak/Snap) nicht anzeigen.

use std::collections::HashMap;

use gtk::prelude::*;
use gtk::{gio, glib};

use crate::config;

const BUS_NAME: &str = "org.freedesktop.impl.portal.PermissionStore";
const OBJECT_PATH: &str = "/org/freedesktop/impl/portal/PermissionStore";
const TABLE: &str = "screenshot";
const ID: &str = "screenshot";

async fn call(method: &str, parameters: glib::Variant) -> Result<glib::Variant, glib::Error> {
    let connection = gio::bus_get_future(gio::BusType::Session).await?;
    connection
        .call_future(
            Some(BUS_NAME),
            OBJECT_PATH,
            BUS_NAME,
            method,
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            2000,
        )
        .await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Granted,
    Denied,
    /// Noch nie gefragt (oder zurückgesetzt): Der nächste Capture zeigt den
    /// Erlaubnis-Dialog – das lässt GNOME nur für die fokussierte App zu.
    Unset,
}

pub async fn state() -> State {
    // Antwort: (a{sas} permissions, v data); Fehler = kein Eintrag vorhanden.
    let Ok(reply) = call("Lookup", (TABLE, ID).to_variant()).await else {
        return State::Unset;
    };
    let values = reply
        .child_value(0)
        .get::<HashMap<String, Vec<String>>>()
        .and_then(|permissions| permissions.get(config::APP_ID).cloned())
        .unwrap_or_default();

    if values.iter().any(|value| value == "yes") {
        State::Granted
    } else if values.iter().any(|value| value == "no") {
        State::Denied
    } else {
        State::Unset
    }
}

/// Löscht Porthole's Eintrag; der nächste Capture fragt wieder nach.
pub async fn reset() -> Result<(), glib::Error> {
    call("DeletePermission", (TABLE, ID, config::APP_ID).to_variant())
        .await
        .map(|_| ())
}
