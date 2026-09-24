use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::capture::portal_screenshot::PortalScreenshotBackend;
use crate::capture::{permission, CaptureBackend, CaptureError};
use crate::config;
use crate::geometry::PixelRect;
use crate::i18n::tr;
use crate::monitors;
use crate::screenshot::Screenshot;
use crate::ui::{main_window, overlay, preview};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Capturing,
    Selecting,
    Cropping,
}

/// Orchestrates a capture run. Triggers that arrive while a run is in progress
/// are dropped (the shell only allows one screenshot at a time anyway). Open
/// previews are not part of the controller's state: they live independently,
/// any number of them in parallel.
pub struct CaptureController {
    app: glib::WeakRef<adw::Application>,
    backend: PortalScreenshotBackend,
    state: Cell<State>,
    restore_main_window: Cell<bool>,
}

/// How long GNOME takes to hide a window; any earlier and it would still be in the shot.
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

    /// `hide_main_window`: the trigger was the button in our own window. For
    /// the shortcut/tray it stays put – hiding it costs 300 ms.
    pub fn request_capture(self: &Rc<Self>, hide_main_window: bool) {
        if self.state.get() != State::Idle {
            glib::g_message!(
                config::LOG_DOMAIN,
                "Capture ignored, state is {:?}",
                self.state.get()
            );
            return;
        }
        self.state.set(State::Capturing);
        let this = self.clone();
        glib::MainContext::default().spawn_local(async move {
            // Without a granted permission the window must keep focus,
            // otherwise GNOME isn't allowed to show the permission dialog.
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
        let (frame, started) = loop {
            let started = Instant::now();
            let result = self.backend.capture_desktop().await;
            let Some(app) = self.app.upgrade() else {
                return;
            };
            match result {
                // Right after the approval the window with the hint is still
                // in the shot: now that it's hidden, take it again (no dialog
                // any more).
                Ok(_) if main_window::capture_succeeded() => {
                    glib::timeout_future(HIDE_ANIMATION).await;
                }
                Ok(frame) => break (frame, started),
                Err(err) => {
                    self.set_idle();
                    report_error(&app, &err);
                    return;
                }
            }
        };
        let Some(app) = self.app.upgrade() else {
            return;
        };
        glib::g_message!(
            config::LOG_DOMAIN,
            "Capture {}x{}: portal {} ms, loading {} ms, total {} ms",
            frame.texture.width(),
            frame.texture.height(),
            frame.portal_time.as_millis(),
            frame.load_time.as_millis(),
            started.elapsed().as_millis()
        );

        let layout = monitors::query().await;
        if layout.is_none() {
            glib::g_message!(config::LOG_DOMAIN, "No Mutter layout, using GDK geometry");
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
            // The large desktop frame is no longer needed from here on.
            drop(frame);
            self.set_idle();

            let Some(app) = self.app.upgrade() else {
                return;
            };
            match screenshot {
                Some(screenshot) => {
                    glib::g_message!(
                        config::LOG_DOMAIN,
                        "Region {}x{} @ ({}, {}): crop + PNG {} ms, {} KiB",
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
                    &CaptureError::Failed(tr("The region could not be created").into()),
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
            // GNOME only needs our window focused to ask: retry by ourselves
            // as soon as it is. Already focused means the portal refused for
            // another reason – then the hint and camera button stay.
            let was_active = main_window::is_active();
            main_window::present_for_approval(app, &err.to_string());
            if !was_active {
                main_window::when_active(glib::clone!(
                    #[weak]
                    app,
                    move || app.activate_action("capture-from-window", None)
                ));
            }
            return;
        }
        _ => {}
    }
    let notification = gio::Notification::new(config::APP_NAME);
    notification.set_body(Some(&err.to_string()));
    app.send_notification(Some("capture-error"), &notification);
}
