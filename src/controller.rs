use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::capture::portal_screenshot::PortalScreenshotBackend;
use crate::capture::{permission, CaptureBackend, CaptureError};
use crate::config;
use crate::geometry::PixelRect;
use crate::monitors;
use crate::screenshot::Screenshot;
use crate::ui::{main_window, overlay, preview};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    /// Portal-Aufruf läuft. Liegt bewusst *vor* der Auswahl: erst wird der
    /// Desktop eingefroren, dann darauf ausgewählt – so kann das Overlay nie
    /// im Bild landen.
    Capturing,
    Selecting,
    Cropping,
}

/// Orchestriert einen Capture-Durchlauf. Auslöser, die während eines laufenden
/// Durchlaufs eintreffen, werden verworfen (die Shell erlaubt ohnehin nur einen
/// Screenshot gleichzeitig). Offene Previews sind kein Zustand des Controllers:
/// sie leben unabhängig, beliebig viele parallel.
pub struct CaptureController {
    app: glib::WeakRef<adw::Application>,
    backend: PortalScreenshotBackend,
    state: Cell<State>,
    /// Hauptfenster wurde für diesen Durchlauf versteckt und kommt danach zurück.
    restore_main_window: Cell<bool>,
}

/// So lange blendet GNOME ein Fenster aus; vorher wäre es noch im Bild.
const HIDE_ANIMATION: Duration = Duration::from_millis(300);

impl CaptureController {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        Rc::new(Self {
            app: app.downgrade(),
            backend: PortalScreenshotBackend,
            state: Cell::new(State::Idle),
            restore_main_window: Cell::new(false),
        })
    }

    /// `hide_main_window`: Auslöser war der Button im eigenen Fenster. Bei
    /// Tastenkürzel/Tray bleibt es stehen – das Verstecken kostet 300 ms.
    pub fn request_capture(self: &Rc<Self>, hide_main_window: bool) {
        if self.state.get() != State::Idle {
            glib::g_message!(
                config::LOG_DOMAIN,
                "Capture ignoriert, Zustand ist {:?}",
                self.state.get()
            );
            return;
        }
        self.state.set(State::Capturing);
        let this = self.clone();
        glib::MainContext::default().spawn_local(async move {
            // Ohne erteilte Berechtigung muss das Fenster fokussiert bleiben,
            // sonst darf GNOME den Erlaubnis-Dialog nicht zeigen.
            if hide_main_window
                && permission::state().await == permission::State::Granted
                && main_window::hide_for_capture()
            {
                this.restore_main_window.set(true);
                glib::timeout_future(HIDE_ANIMATION).await;
            }
            this.capture().await;
        });
    }

    fn set_idle(&self) {
        self.state.set(State::Idle);
        if self.restore_main_window.replace(false) {
            main_window::restore_after_capture();
        }
    }

    async fn capture(self: Rc<Self>) {
        let started = Instant::now();
        let result = self.backend.capture_desktop().await;
        let Some(app) = self.app.upgrade() else {
            return;
        };

        let frame = match result {
            Ok(frame) => frame,
            Err(err) => {
                self.set_idle();
                report_error(&app, &err);
                return;
            }
        };
        glib::g_message!(
            config::LOG_DOMAIN,
            "Capture {}x{}: Portal {} ms, Laden {} ms, gesamt {} ms",
            frame.texture.width(),
            frame.texture.height(),
            frame.portal_time.as_millis(),
            frame.load_time.as_millis(),
            started.elapsed().as_millis()
        );
        main_window::set_hint(None);

        let layout = monitors::query().await;
        if layout.is_none() {
            glib::g_message!(config::LOG_DOMAIN, "Kein Mutter-Layout, nutze GDK-Geometrie");
        }

        self.state.set(State::Selecting);
        let this = self.clone();
        let texture = frame.texture.clone();
        let on_done = move |outcome| match outcome {
            overlay::Outcome::Selected(rect) => this.finish_selection(texture, rect),
            overlay::Outcome::Cancelled => this.set_idle(),
        };
        if let Err(detail) = overlay::present(&app, &frame.texture, layout.as_ref(), on_done) {
            self.set_idle();
            report_error(&app, &CaptureError::Failed(detail));
        }
    }

    fn finish_selection(self: Rc<Self>, frame: gdk::Texture, rect: PixelRect) {
        self.state.set(State::Cropping);
        glib::MainContext::default().spawn_local(async move {
            let started = Instant::now();
            let screenshot = Screenshot::from_frame(&frame, rect).await;
            // Der große Desktop-Frame wird ab hier nicht mehr gebraucht.
            drop(frame);
            self.set_idle();

            let Some(app) = self.app.upgrade() else {
                return;
            };
            match screenshot {
                Some(screenshot) => {
                    glib::g_message!(
                        config::LOG_DOMAIN,
                        "Ausschnitt {}x{} @ ({}, {}): Zuschnitt + PNG {} ms, {} KiB",
                        rect.width,
                        rect.height,
                        rect.x,
                        rect.y,
                        started.elapsed().as_millis(),
                        screenshot.png.len() / 1024
                    );
                    preview::present(&app, screenshot);
                }
                None => report_error(
                    &app,
                    &CaptureError::Failed("Ausschnitt konnte nicht erzeugt werden".into()),
                ),
            }
        });
    }
}

fn report_error(app: &adw::Application, err: &CaptureError) {
    glib::g_warning!(config::LOG_DOMAIN, "{err}");
    match err {
        CaptureError::Cancelled => return,
        CaptureError::PermissionNeedsFocus => {
            main_window::present_with_hint(app, &err.to_string());
            return;
        }
        _ => {}
    }
    let notification = gio::Notification::new(config::APP_NAME);
    notification.set_body(Some(&err.to_string()));
    app.send_notification(Some("capture-error"), &notification);
}
