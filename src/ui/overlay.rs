use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

use super::selection_area::SelectionArea;
use crate::geometry::{self, PixelRect, Rect};
use crate::i18n::tr;
use crate::monitors::StageLayout;
use crate::screenshot;

/// Smaller selections (in image pixels) count as an accidental click.
const MIN_SELECTION: i32 = 3;

pub enum Outcome {
    Selected(PixelRect),
    Cancelled,
}

/// One selection run: a fullscreen window per monitor showing its part of the
/// frozen desktop.
///
/// The windows are recreated on every run: under Mutter only a freshly mapped
/// toplevel reliably gets keyboard focus when the trigger (global shortcut)
/// came from the background.
///
/// The selection lives in stage space and may span several monitors: during
/// a drag, Wayland also delivers coordinates outside the starting window,
/// which are converted to the stage here and then distributed to all
/// monitors.
struct Session {
    windows: RefCell<Vec<gtk::Window>>,
    /// Per monitor: display widget and its rectangle in stage space.
    monitors: Vec<(SelectionArea, Rect)>,
    /// Bounding box of all monitors; corresponds to the whole image.
    stage: Rect,
    image: PixelRect,
    /// Start point of the current drag in stage space.
    drag_start: Cell<(f64, f64)>,
    selection: Cell<Option<Rect>>,
    on_done: RefCell<Option<DoneCallback>>,
}

type DoneCallback = Box<dyn FnOnce(Outcome)>;

impl Session {
    fn finish(&self, outcome: Outcome) {
        // Only the first call counts (ESC, right click and focus loss can
        // coincide).
        let Some(on_done) = self.on_done.take() else {
            return;
        };
        for window in self.windows.take() {
            window.destroy();
        }
        on_done(outcome);
    }

    /// Window coordinates of monitor `index` → stage. Also valid for points
    /// outside the window (Wayland reports them relative to the start window).
    fn to_stage(&self, index: usize, (x, y): (f64, f64)) -> (f64, f64) {
        let (area, rect) = &self.monitors[index];
        let scale_x = rect.width / f64::from(area.width().max(1));
        let scale_y = rect.height / f64::from(area.height().max(1));
        (rect.x + x * scale_x, rect.y + y * scale_y)
    }

    fn to_pixels(&self, selection: Rect) -> PixelRect {
        let relative = Rect::new(
            selection.x - self.stage.x,
            selection.y - self.stage.y,
            selection.width,
            selection.height,
        );
        geometry::selection_to_pixels(relative, (self.stage.width, self.stage.height), self.image)
    }

    fn set_selection(&self, selection: Option<Rect>) {
        self.selection.set(selection);

        let parts: Vec<Option<Rect>> = self
            .monitors
            .iter()
            .map(|(area, rect)| {
                let part = selection?.intersection(*rect)?;
                let scale_x = f64::from(area.width()) / rect.width;
                let scale_y = f64::from(area.height()) / rect.height;
                Some(Rect::new(
                    (part.x - rect.x) * scale_x,
                    (part.y - rect.y) * scale_y,
                    part.width * scale_x,
                    part.height * scale_y,
                ))
            })
            .collect();

        // The size goes below the bottom-left corner, i.e. on the monitor
        // containing it (otherwise on the first one involved).
        let label_owner = selection.and_then(|selection| {
            let corner = (selection.x, selection.y + selection.height);
            self.monitors
                .iter()
                .zip(&parts)
                .position(|((_, rect), part)| part.is_some() && rect.contains(corner))
                .or_else(|| parts.iter().position(Option::is_some))
        });
        let label = selection.map(|selection| {
            let pixels = self.to_pixels(selection);
            format!("{} × {}", pixels.width, pixels.height)
        });

        for (index, ((area, _), part)) in self.monitors.iter().zip(parts).enumerate() {
            let label = (label_owner == Some(index)).then(|| label.clone()).flatten();
            area.set_selection(part, label);
        }
    }

    fn cancel_if_unfocused(&self) {
        let any_active = self.windows.borrow().iter().any(|window| window.is_active());
        if !any_active {
            self.finish(Outcome::Cancelled);
        }
    }
}

/// `layout`: stage rectangles from Mutter; if it (or a connector in it) is
/// missing, GDK's monitor geometry serves as an approximation.
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
        .map_err(|err| {
            tr("Monitor layout doesn't match the screenshot: {error}")
                .replace("{error}", &format!("{err:?}"))
        })?;

    let stage = stage_rects
        .iter()
        .copied()
        .reduce(Rect::union)
        .ok_or("Keine Monitore")?;
    let session = Rc::new(Session {
        windows: RefCell::new(Vec::new()),
        monitors: regions
            .iter()
            .zip(&stage_rects)
            .map(|(region, rect)| (SelectionArea::new(&screenshot::view(frame, *region)), *rect))
            .collect(),
        stage,
        image: PixelRect::new(0, 0, frame.width(), frame.height()),
        drag_start: Cell::new((0.0, 0.0)),
        selection: Cell::new(None),
        on_done: RefCell::new(Some(Box::new(on_done))),
    });

    for (index, monitor) in monitors.iter().enumerate() {
        let window = build_window(app, &session, index);
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
    index: usize,
) -> gtk::Window {
    let area = session.monitors[index].0.clone();
    // The session owns the areas, so their gestures may only reference it
    // weakly – otherwise the whole frame would stay in RAM after the run.

    let window = gtk::Window::builder()
        .application(app)
        .decorated(false)
        .child(&area)
        .build();
    window.set_cursor_from_name(Some("crosshair"));

    let drag = gtk::GestureDrag::builder().button(gdk::BUTTON_PRIMARY).build();
    drag.connect_drag_begin(glib::clone!(
        #[weak]
        session,
        move |_, x, y| session.drag_start.set(session.to_stage(index, (x, y)))
    ));
    drag.connect_drag_update(glib::clone!(
        #[weak]
        session,
        move |gesture, offset_x, offset_y| {
            let Some((x, y)) = gesture.start_point() else {
                return;
            };
            let end = session.to_stage(index, (x + offset_x, y + offset_y));
            let selection =
                Rect::from_points(session.drag_start.get(), end).clamp_to(session.stage);
            session.set_selection(Some(selection));
        }
    ));
    drag.connect_drag_end(glib::clone!(
        #[weak]
        session,
        move |_, _, _| {
            let Some(selection) = session.selection.get() else {
                return;
            };
            let pixels = session.to_pixels(selection);
            if pixels.width < MIN_SELECTION || pixels.height < MIN_SELECTION {
                // Accidental click: stay in selection mode.
                session.set_selection(None);
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
        #[weak]
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

    // Focus lost (Alt+Tab, Super key …) → cancel. Check only on the next
    // main loop iteration: with several monitors, focus may just move to
    // another overlay window.
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
