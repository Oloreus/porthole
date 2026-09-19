//! Globales Tastenkürzel über `org.freedesktop.portal.GlobalShortcuts`.
//!
//! Unter Wayland darf kein Client Tasten global abgreifen; die App meldet ihren
//! Wunsch beim Portal an, GNOME fragt den Nutzer einmalig („Tastenkürzel
//! hinzufügen") und die Shell hält den eigentlichen Grab. Ändern lässt sich das
//! Kürzel danach unter Einstellungen → Apps → Porthole → Globale Tastenkürzel.

use adw::prelude::*;
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_util::StreamExt;
use gtk::glib;

use crate::config;

/// Stabil halten: GNOME zeigt den Bestätigungsdialog nur für IDs, die es
/// noch nicht kennt.
const CAPTURE_ID: &str = "capture";
/// Shortcuts-Spec: Modifier groß, Keysym klein ("ALT+S" wäre Alt+Shift+S).
const DEFAULT_TRIGGER: &str = "ALT+s";

/// Bindet das Kürzel und leitet Auslösungen an `app.capture` weiter. Läuft für
/// die gesamte Prozesslaufzeit – endet die Session, endet der Grab.
pub async fn run(app: adw::Application) {
    if let Err(err) = listen(&app).await {
        glib::g_warning!(
            config::LOG_DOMAIN,
            "Globales Tastenkürzel nicht verfügbar: {err}. Ausweichlösung: in den \
             GNOME-Tastatureinstellungen ein eigenes Kürzel mit dem Befehl \
             „porthole --capture“ anlegen."
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
