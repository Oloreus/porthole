//! Globales Tastenkürzel über `org.freedesktop.portal.GlobalShortcuts`.
//!
//! Unter Wayland darf kein Client Tasten global abgreifen; die App meldet ihren
//! Wunsch beim Portal an, GNOME fragt den Nutzer einmalig („Tastenkürzel
//! hinzufügen") und die Shell hält den eigentlichen Grab. Ändern lässt sich das
//! Kürzel danach unter Einstellungen → Apps → Porthole → Globale Tastenkürzel.

use std::fs;
use std::path::PathBuf;

use adw::prelude::*;
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_util::StreamExt;
use gtk::glib;

use crate::config;
use crate::ui::main_window;

/// Stabil halten: GNOME zeigt den Bestätigungsdialog nur für IDs, die es
/// noch nicht kennt.
const CAPTURE_ID: &str = "capture";
/// Shortcuts-Spec: Modifier groß, Keysym klein ("ALT+S" wäre Alt+Shift+S).
const DEFAULT_TRIGGER: &str = "ALT+s";

/// Bindet das Kürzel und leitet Auslösungen an `app.capture` weiter. Läuft für
/// die gesamte Prozesslaufzeit – endet die Session, endet der Grab.
pub async fn run(app: adw::Application) {
    match listen(&app).await {
        Ok(()) => {}
        // GNOME < 48 (z. B. Ubuntu 24.04) hat kein GlobalShortcuts-Backend.
        Err(ashpd::Error::PortalNotFound(_)) => {
            glib::g_message!(
                config::LOG_DOMAIN,
                "GlobalShortcuts-Portal fehlt (GNOME < 48) – Kürzel muss in den \
                 Tastatureinstellungen mit „porthole --capture“ angelegt werden"
            );
            show_missing_portal_hint_once(&app);
        }
        Err(err) => glib::g_warning!(
            config::LOG_DOMAIN,
            "Globales Tastenkürzel nicht verfügbar: {err}. Ausweichlösung: in den \
             GNOME-Tastatureinstellungen ein eigenes Kürzel mit dem Befehl \
             „porthole --capture“ anlegen."
        ),
    }
}

/// Holt beim ersten Start das Fenster mit Anleitung nach vorn; danach steht
/// der Hinweis nur noch im Log und im README.
fn show_missing_portal_hint_once(app: &adw::Application) {
    let Some(marker) = hint_marker_path() else {
        return;
    };
    if marker.exists() {
        return;
    }
    main_window::present_with_hint_action(
        app,
        "Globales Tastenkürzel braucht GNOME 48. Bitte unter Tastatur → Eigene \
         Tastenkürzel den Befehl „porthole --capture“ anlegen.",
        "Öffnen",
        "app.open-keyboard-settings",
    );
    let written = marker
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&marker, b""));
    if let Err(err) = written {
        glib::g_warning!(
            config::LOG_DOMAIN,
            "Konnte {} nicht anlegen: {err}",
            marker.display()
        );
    }
}

fn hint_marker_path() -> Option<PathBuf> {
    let state_dir = glib::user_state_dir();
    state_dir.is_absolute().then(|| {
        state_dir
            .join(config::LOG_DOMAIN)
            .join("shortcut-hint-shown")
    })
}

/// Öffnet die GNOME-Tastatureinstellungen (dort: „Eigene Tastenkürzel“).
pub fn open_keyboard_settings() {
    if let Err(err) = glib::spawn_command_line_async("gnome-control-center keyboard") {
        glib::g_warning!(
            config::LOG_DOMAIN,
            "Tastatureinstellungen nicht startbar: {err}"
        );
    }
}

async fn listen(app: &adw::Application) -> ashpd::Result<()> {
    let portal = GlobalShortcuts::new().await?;
    let session = portal.create_session(Default::default()).await?;

    let capture = NewShortcut::new(CAPTURE_ID, "Bereichs-Screenshot aufnehmen")
        .preferred_trigger(DEFAULT_TRIGGER);
    // Bei jedem Start nötig (der Grab lebt nur mit der Session); den Dialog
    // zeigt GNOME aber nur beim ersten Mal.
    let request = portal
        .bind_shortcuts(&session, &[capture], None, Default::default())
        .await?;
    match request.response() {
        Ok(bound) => {
            for shortcut in bound.shortcuts() {
                glib::g_message!(
                    config::LOG_DOMAIN,
                    "Globales Tastenkürzel „{}“: {}",
                    shortcut.id(),
                    shortcut.trigger_description()
                );
            }
        }
        // xdg-desktop-portal-gnome 48.0 antwortet mit Code 2 ("Fehler"),
        // obwohl die Shell den Grab vergeben hat und die Session steht.
        // Deshalb nicht abbrechen, sondern trotzdem lauschen. Hat der Nutzer
        // den Dialog wirklich abgelehnt, kommt schlicht nie ein Signal.
        Err(ashpd::Error::Response(response)) => glib::g_message!(
            config::LOG_DOMAIN,
            "BindShortcuts meldet „{response}“ – lausche trotzdem auf das Tastenkürzel"
        ),
        Err(err) => return Err(err),
    }

    let mut activations = portal.receive_activated().await?;
    while let Some(activation) = activations.next().await {
        glib::g_message!(
            config::LOG_DOMAIN,
            "Tastenkürzel ausgelöst: {}",
            activation.shortcut_id()
        );
        if activation.shortcut_id() == CAPTURE_ID {
            app.activate_action("capture", None);
        }
    }

    // Stream beendet = Portal weg. `session` bis hierher am Leben halten.
    drop(session);
    Ok(())
}
