use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

use super::selection_area::SelectionArea;
use crate::geometry::{self, PixelRect, Rect};
use crate::monitors::StageLayout;
use crate::screenshot;

/// Kleinere Auswahlen (in Bildpixeln) gelten als versehentlicher Klick.
const MIN_SELECTION: i32 = 3;

pub enum Outcome {
    Selected(PixelRect),
    Cancelled,
}

/// Ein Auswahl-Durchlauf: je Monitor ein Vollbildfenster mit seinem Teil des
/// eingefrorenen Desktops.
///
/// Die Fenster werden bei jedem Durchlauf neu erzeugt: Nur ein frisch
/// gemapptes Toplevel bekommt unter Mutter zuverlässig den Tastaturfokus, wenn
/// der Auslöser (globaler Shortcut) aus dem Hintergrund kam.
struct Session {
    windows: RefCell<Vec<gtk::Window>>,
    on_done: RefCell<Option<DoneCallback>>,
}

type DoneCallback = Box<dyn FnOnce(Outcome)>;

impl Session {
    fn finish(&self, outcome: Outcome) {
        // Nur der erste Aufruf zählt (ESC, Rechtsklick und Fokusverlust
        // können zusammenfallen).
        let Some(on_done) = self.on_done.take() else {
            return;
        };
        for window in self.windows.take() {
            window.destroy();
        }
        on_done(outcome);
    }

    fn cancel_if_unfocused(&self) {
        let any_active = self.windows.borrow().iter().any(|window| window.is_active());
        if !any_active {
            self.finish(Outcome::Cancelled);
        }
    }
}

/// `layout`: Stage-Rechtecke von Mutter; fehlt es (oder ein Anschluss darin),
/// dient GDKs Monitor-Geometrie als Näherung.
pub fn present(
    app: &impl IsA<gtk::Application>,
    frame: &gdk::Texture,
    layout: Option<&StageLayout>,
    on_done: impl FnOnce(Outcome) + 'static,
) -> Result<(), String> {
    let display = gdk::Display::default().ok_or("Kein Display")?;
    let monitors: Vec<gdk::Monitor> = display
        .monitors()
        .iter::<gdk::Monitor>()
        .filter_map(Result::ok)
        .collect();

    let stage_rects: Vec<Rect> = monitors
        .iter()
        .map(|monitor| {
            let from_mutter = monitor
                .connector()
                .and_then(|connector| layout?.get(connector.as_str()).copied());
            if let Some(rect) = from_mutter {
                return rect;
            }
            let geometry = monitor.geometry();
            Rect::new(
                f64::from(geometry.x()),
                f64::from(geometry.y()),
                f64::from(geometry.width()),
                f64::from(geometry.height()),
            )
        })
        .collect();
    let regions = geometry::monitor_regions(&stage_rects, (frame.width(), frame.height()))
        .map_err(|err| format!("Monitor-Layout passt nicht zum Screenshot: {err:?}"))?;

    let session = Rc::new(Session {
        windows: RefCell::new(Vec::new()),
        on_done: RefCell::new(Some(Box::new(on_done))),
    });

    for (monitor, region) in monitors.iter().zip(regions) {
        let window = build_window(app, &session, frame, region);
        window.fullscreen_on_monitor(monitor);
        session.windows.borrow_mut().push(window);
    }
    for window in session.windows.borrow().iter() {
        window.present();
    }
    Ok(())
}

fn build_window(
    app: &impl IsA<gtk::Application>,
    session: &Rc<Session>,
    frame: &gdk::Texture,
    region: PixelRect,
) -> gtk::Window {
    let area = SelectionArea::new(&screenshot::view(frame, region), region);

    let window = gtk::Window::builder()
        .application(app)
        .decorated(false)
        .child(&area)
        .build();
    window.set_cursor_from_name(Some("crosshair"));

    let drag_start = Rc::new(Cell::new((0.0, 0.0)));
    let drag = gtk::GestureDrag::builder().button(gdk::BUTTON_PRIMARY).build();
    drag.connect_drag_begin(glib::clone!(
        #[strong]
        drag_start,
        move |_, x, y| drag_start.set((x, y))
    ));
    drag.connect_drag_update(glib::clone!(
        #[weak]
        area,
        #[strong]
        drag_start,
        move |_, offset_x, offset_y| {
            // Während des Drags liefert Wayland auch Koordinaten außerhalb
            // des Fensters; die Auswahl bleibt auf diesen Monitor begrenzt.
            let (x, y) = drag_start.get();
            let bounds = Rect::new(0.0, 0.0, f64::from(area.width()), f64::from(area.height()));
            let selection =
                Rect::from_points((x, y), (x + offset_x, y + offset_y)).clamp_to(bounds);
            area.set_selection(Some(selection));
        }
    ));
    drag.connect_drag_end(glib::clone!(
        #[weak]
        area,
        #[strong]
        session,
        move |_, _, _| {
            let Some(selection) = area.selection() else {
                return;
            };
            let pixels = geometry::selection_to_pixels(
                selection,
                (f64::from(area.width()), f64::from(area.height())),
                area.region(),
            );
            if pixels.width < MIN_SELECTION || pixels.height < MIN_SELECTION {
                // Versehentlicher Klick: im Auswahlmodus bleiben.
                area.set_selection(None);
            } else {
                session.finish(Outcome::Selected(pixels));
            }
        }
    ));
    area.add_controller(drag);

    let right_click = gtk::GestureClick::builder()
        .button(gdk::BUTTON_SECONDARY)
        .build();
    right_click.connect_pressed(glib::clone!(
        #[strong]
        session,
        move |_, _, _, _| session.finish(Outcome::Cancelled)
    ));
    area.add_controller(right_click);

    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[strong]
        session,
        move |_, key, _, _| {
            if key == gdk::Key::Escape {
                session.finish(Outcome::Cancelled);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    ));
    window.add_controller(keys);

    // Fokus weg (Alt+Tab, Super-Taste …) → abbrechen. Erst im nächsten
    // Mainloop-Durchlauf prüfen: bei mehreren Monitoren wandert der Fokus
    // evtl. nur zu einem anderen Overlay-Fenster.
    window.connect_is_active_notify(glib::clone!(
        #[strong]
        session,
        move |window| {
            if !window.is_active() {
                glib::idle_add_local_once(glib::clone!(
                    #[strong]
                    session,
                    move || session.cancel_if_unfocused()
                ));
            }
        }
    ));

    window
}
