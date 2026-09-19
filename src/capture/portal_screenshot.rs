use std::time::{Duration, Instant};

use ashpd::desktop::screenshot::Screenshot;
use ashpd::desktop::ResponseError;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use super::{permission, CaptureBackend, CaptureError, DesktopFrame};
use crate::config;

/// Großzügig, weil beim allerersten Aufruf der Erlaubnis-Dialog offen ist.
const PORTAL_TIMEOUT: Duration = Duration::from_secs(60);

/// Capture über `org.freedesktop.portal.Screenshot` (nicht interaktiv).
///
/// GNOME-Eigenheiten: Die Shell blitzt und wartet ~500 ms auf die
/// Flash-Animation, und sie legt das Bild als `Screenshot[-N].png` im
/// Bilder-Ordner ab, ohne es je aufzuräumen – das übernehmen wir.
pub struct PortalScreenshotBackend;

impl CaptureBackend for PortalScreenshotBackend {
    async fn capture_desktop(&self) -> Result<DesktopFrame, CaptureError> {
        let started = Instant::now();
        let request = async {
            Screenshot::request()
                .interactive(false)
                .send()
                .await?
                .response()
        };
        let response = glib::future_with_timeout(PORTAL_TIMEOUT, request)
            .await
            .map_err(|_| CaptureError::Timeout)?;
        let portal_time = started.elapsed();

        let screenshot = match response {
            Ok(screenshot) => screenshot,
            Err(err) => return Err(map_error(err).await),
        };

        let started = Instant::now();
        let file = gio::File::for_uri(screenshot.uri().as_str());
        let texture = gdk::Texture::from_file(&file);
        // Immer löschen, auch wenn das Laden scheiterte.
        if let Err(err) = file.delete(gio::Cancellable::NONE) {
            glib::g_warning!(
                config::LOG_DOMAIN,
                "Portal-Datei {} nicht gelöscht: {err}",
                file.uri()
            );
        }
        let texture = texture.map_err(|err| CaptureError::Load(err.to_string()))?;

        Ok(DesktopFrame {
            texture,
            portal_time,
            load_time: started.elapsed(),
        })
    }
}

async fn map_error(err: ashpd::Error) -> CaptureError {
    match err {
        ashpd::Error::Response(ResponseError::Cancelled) => CaptureError::Cancelled,
        // XDP meldet Berechtigungsprobleme nur als generisches "Other".
        ashpd::Error::Response(ResponseError::Other) => match permission::state().await {
            permission::State::Denied => CaptureError::PermissionDenied,
            // Der Erlaubnis-Dialog wurde gar nicht erst gezeigt: gnome-shell
            // lässt ihn nur zu, wenn ein Fenster der App den Fokus hat.
            permission::State::Unset => CaptureError::PermissionNeedsFocus,
            permission::State::Granted => {
                CaptureError::Failed("Portal meldet einen Fehler (Response 2)".into())
            }
        },
        ashpd::Error::PortalNotFound(_) | ashpd::Error::Zbus(_) => {
            CaptureError::PortalUnavailable(err.to_string())
        }
        other => CaptureError::Failed(other.to_string()),
    }
}
