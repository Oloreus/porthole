//! Screenshot-Capture, strikt getrennt von der UI: Backends liefern ein Bild
//! des gesamten Desktops, Auswahl und Zuschnitt passieren danach lokal.

pub mod permission;
pub mod portal_screenshot;

use std::fmt;
use std::time::Duration;

use gtk::gdk;

/// Bild des gesamten Desktops (alle Monitore, ein Bild) plus Messwerte.
pub struct DesktopFrame {
    pub texture: gdk::Texture,
    /// Dauer des Portal-Aufrufs bis zur Antwort.
    pub portal_time: Duration,
    /// Dauer fürs Laden/Dekodieren des Bildes.
    pub load_time: Duration,
}

#[derive(Debug)]
pub enum CaptureError {
    /// Der Nutzer hat Porthole das Aufnehmen verboten. XDP fragt danach nie
    /// wieder von selbst – nur `permission::reset` hebt das auf.
    PermissionDenied,
    /// Noch keine Berechtigung, und der Erlaubnis-Dialog durfte nicht
    /// erscheinen, weil kein Porthole-Fenster fokussiert war.
    PermissionNeedsFocus,
    Cancelled,
    PortalUnavailable(String),
    Timeout,
    Load(String),
    Failed(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied => write!(
                f,
                "Porthole darf keine Screenshots aufnehmen. Berechtigung zurücksetzen mit: \
                 porthole --reset-permission"
            ),
            Self::PermissionNeedsFocus => write!(
                f,
                "Einmalige Freigabe nötig: Bitte hier auf den Kamera-Button klicken und \
                 „Erlauben“ wählen. Danach funktioniert das Tastenkürzel von überall."
            ),
            Self::Cancelled => write!(f, "Aufnahme abgebrochen."),
            Self::PortalUnavailable(detail) => {
                write!(f, "Screenshot-Portal nicht verfügbar: {detail}")
            }
            Self::Timeout => write!(f, "Das Screenshot-Portal hat nicht rechtzeitig geantwortet."),
            Self::Load(detail) => write!(f, "Screenshot konnte nicht geladen werden: {detail}"),
            Self::Failed(detail) => write!(f, "Aufnahme fehlgeschlagen: {detail}"),
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait CaptureBackend {
    async fn capture_desktop(&self) -> Result<DesktopFrame, CaptureError>;
}
