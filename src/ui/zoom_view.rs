use gtk::glib;
use gtk::subclass::prelude::*;

mod imp {
    use std::cell::{Cell, RefCell};

    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{gdk, glib, graphene, gsk};

    use crate::viewport::{Viewport, ZoomMode, ZOOM_STEP};

    /// Pan-Weg je Mausrad-Raste.
    const WHEEL_PAN_STEP: f64 = 48.0;
    /// Touchpads liefern Pixel statt Rasten; so viele entsprechen einem
    /// Zoomschritt.
    const TOUCHPAD_PIXELS_PER_STEP: f64 = 50.0;

    type ZoomCallback = Box<dyn Fn(f64)>;

    #[derive(Default)]
    pub struct ZoomView {
        pub texture: RefCell<Option<gdk::Texture>>,
        pub viewport: RefCell<Viewport>,
        /// Letzte Zeigerposition – das Scroll-Ereignis selbst kennt keine.
        pointer: Cell<Option<(f64, f64)>>,
        /// Pan beim Beginn des Ziehens; `Some` heißt: es wird gezogen.
        drag_start_pan: Cell<Option<(f64, f64)>>,
        pub on_zoom_changed: RefCell<Option<ZoomCallback>>,
        notified_zoom: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ZoomView {
        const NAME: &'static str = "PortholeZoomView";
        type Type = super::ZoomView;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ZoomView {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            obj.connect_scale_factor_notify(|obj| obj.queue_allocate());
            self.install_controllers();
        }
    }

    impl WidgetImpl for ZoomView {
        fn realize(&self) {
            self.parent_realize();
            // Fractional Scaling ändert den Surface-Scale, ohne dass sich der
            // ganzzahlige `scale-factor` ändern muss.
            if let Some(surface) = self.obj().native().and_then(|native| native.surface()) {
                surface.connect_scale_notify(glib::clone!(
                    #[weak(rename_to = obj)]
                    self.obj(),
                    move |_| obj.queue_allocate()
                ));
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);

            let scale = self.surface_scale();
            {
                let texture = self.texture.borrow();
                let mut viewport = self.viewport.borrow_mut();
                if let Some(texture) = texture.as_ref() {
                    viewport.set_image(
                        f64::from(texture.width()) / scale,
                        f64::from(texture.height()) / scale,
                    );
                }
                viewport.set_size(f64::from(width), f64::from(height));
            }
            self.update_cursor();

            // Der Empfänger ändert ein Label; das darf nicht mitten in der
            // Größenzuteilung passieren.
            if self.viewport.borrow().zoom() != self.notified_zoom.get() {
                glib::idle_add_local_once(glib::clone!(
                    #[weak(rename_to = obj)]
                    self.obj(),
                    move || obj.imp().notify_zoom()
                ));
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let texture = self.texture.borrow();
            let Some(texture) = texture.as_ref() else {
                return;
            };
            let obj = self.obj();
            let viewport = self.viewport.borrow();
            let rect = viewport.image_rect();

            // Auf Gerätepixel ausrichten, sonst ist 100 % nicht pixelgenau.
            let scale = self.surface_scale();
            let align = |value: f64| ((value * scale).round() / scale) as f32;
            let bounds = graphene::Rect::new(
                align(rect.x),
                align(rect.y),
                rect.width as f32,
                rect.height as f32,
            );

            // Verkleinert mit Mipmaps; stark vergrößert bleiben die Pixel
            // scharf, statt zu verschwimmen.
            let zoom = viewport.zoom();
            let filter = if zoom < 1.0 {
                gsk::ScalingFilter::Trilinear
            } else if zoom < 2.0 {
                gsk::ScalingFilter::Linear
            } else {
                gsk::ScalingFilter::Nearest
            };

            let clip = graphene::Rect::new(0.0, 0.0, obj.width() as f32, obj.height() as f32);
            snapshot.push_clip(&clip);
            snapshot.append_scaled_texture(texture, filter, &bounds);
            snapshot.pop();
        }
    }

    impl ZoomView {
        fn install_controllers(&self) {
            let obj = self.obj();

            let motion = gtk::EventControllerMotion::new();
            motion.connect_enter(glib::clone!(
                #[weak]
                obj,
                move |_, x, y| obj.imp().pointer.set(Some((x, y)))
            ));
            motion.connect_motion(glib::clone!(
                #[weak]
                obj,
                move |_, x, y| obj.imp().pointer.set(Some((x, y)))
            ));
            motion.connect_leave(glib::clone!(
                #[weak]
                obj,
                move |_| obj.imp().pointer.set(None)
            ));
            obj.add_controller(motion);

            let scroll =
                gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
            scroll.connect_scroll(glib::clone!(
                #[weak]
                obj,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |scroll, dx, dy| obj.imp().on_scroll(scroll, dx, dy)
            ));
            obj.add_controller(scroll);

            let drag = gtk::GestureDrag::builder().button(gdk::BUTTON_PRIMARY).build();
            drag.connect_drag_begin(glib::clone!(
                #[weak]
                obj,
                move |_, _, _| {
                    let imp = obj.imp();
                    let pan = {
                        let viewport = imp.viewport.borrow();
                        viewport.is_pannable().then(|| viewport.pan())
                    };
                    if pan.is_some() {
                        imp.drag_start_pan.set(pan);
                        imp.update_cursor();
                    }
                }
            ));
            drag.connect_drag_update(glib::clone!(
                #[weak]
                obj,
                move |_, offset_x, offset_y| {
                    let imp = obj.imp();
                    if let Some((x, y)) = imp.drag_start_pan.get() {
                        imp.viewport.borrow_mut().pan_to(x + offset_x, y + offset_y);
                        imp.changed();
                    }
                }
            ));
            drag.connect_drag_end(glib::clone!(
                #[weak]
                obj,
                move |_, _, _| obj.imp().end_drag()
            ));
            drag.connect_cancel(glib::clone!(
                #[weak]
                obj,
                move |_, _| obj.imp().end_drag()
            ));
            obj.add_controller(drag);

            let double_click = gtk::GestureClick::builder().button(gdk::BUTTON_PRIMARY).build();
            double_click.connect_pressed(glib::clone!(
                #[weak]
                obj,
                move |_, n_press, x, y| {
                    if n_press == 2 {
                        obj.imp().toggle_zoom((x, y));
                    }
                }
            ));
            obj.add_controller(double_click);
        }

        fn on_scroll(
            &self,
            scroll: &gtk::EventControllerScroll,
            dx: f64,
            dy: f64,
        ) -> glib::Propagation {
            let state = scroll.current_event_state();
            let in_pixels = scroll.unit() == gdk::ScrollUnit::Surface;

            if state.contains(gdk::ModifierType::CONTROL_MASK) {
                let delta = if dy != 0.0 { dy } else { dx };
                let steps = if in_pixels { delta / TOUCHPAD_PIXELS_PER_STEP } else { delta };
                let anchor = self.pointer.get().unwrap_or_else(|| self.center());
                self.viewport.borrow_mut().zoom_at(anchor, ZOOM_STEP.powf(-steps));
                self.changed();
                return glib::Propagation::Stop;
            }

            // Ohne Strg: nur verschieben, wenn es etwas zu verschieben gibt –
            // sonst bleibt das Mausrad wirkungslos wie zuvor.
            if !self.viewport.borrow().is_pannable() {
                return glib::Propagation::Proceed;
            }
            let (mut dx, mut dy) = if in_pixels {
                (dx, dy)
            } else {
                (dx * WHEEL_PAN_STEP, dy * WHEEL_PAN_STEP)
            };
            if state.contains(gdk::ModifierType::SHIFT_MASK) && dx == 0.0 {
                (dx, dy) = (dy, 0.0);
            }
            self.viewport.borrow_mut().pan_by(-dx, -dy);
            self.changed();
            glib::Propagation::Stop
        }

        /// Doppelklick: Einpassen ↔ 100 %. Ist das Bild bei 100 % ohnehin
        /// ganz sichtbar, stattdessen 200 %.
        fn toggle_zoom(&self, anchor: (f64, f64)) {
            {
                let mut viewport = self.viewport.borrow_mut();
                if viewport.mode() == ZoomMode::Fit {
                    let target = if viewport.fit_zoom() < 1.0 { 1.0 } else { 2.0 };
                    viewport.set_zoom_at(anchor, target);
                } else {
                    viewport.set_fit();
                }
            }
            self.changed();
        }

        fn end_drag(&self) {
            self.drag_start_pan.set(None);
            self.update_cursor();
        }

        pub fn center(&self) -> (f64, f64) {
            let obj = self.obj();
            (f64::from(obj.width()) / 2.0, f64::from(obj.height()) / 2.0)
        }

        /// Nach jeder Änderung von Zoom oder Pan.
        pub fn changed(&self) {
            self.obj().queue_draw();
            self.update_cursor();
            self.notify_zoom();
        }

        fn update_cursor(&self) {
            let cursor = if self.drag_start_pan.get().is_some() {
                Some("grabbing")
            } else if self.viewport.borrow().is_pannable() {
                Some("grab")
            } else {
                None
            };
            self.obj().set_cursor_from_name(cursor);
        }

        fn notify_zoom(&self) {
            let zoom = self.viewport.borrow().zoom();
            if self.notified_zoom.replace(zoom) == zoom {
                return;
            }
            if let Some(on_zoom_changed) = self.on_zoom_changed.borrow().as_ref() {
                on_zoom_changed(zoom);
            }
        }

        fn surface_scale(&self) -> f64 {
            let obj = self.obj();
            obj.native()
                .and_then(|native| native.surface())
                .map(|surface| surface.scale())
                .filter(|scale| *scale > 0.0)
                .unwrap_or_else(|| f64::from(obj.scale_factor()))
        }
    }
}

glib::wrapper! {
    /// Zeigt eine Textur mit Zoom (Strg+Mausrad, am Zeiger verankert) und Pan
    /// (Ziehen, Mausrad). Reine Darstellung: die Textur wird nie verändert,
    /// skaliert wird je Frame auf der GPU. Die Mathematik liegt in
    /// [`crate::viewport`].
    pub struct ZoomView(ObjectSubclass<imp::ZoomView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ZoomView {
    pub fn new(texture: &gtk::gdk::Texture) -> Self {
        let view: Self = glib::Object::new();
        view.imp().texture.replace(Some(texture.clone()));
        view
    }

    /// Zurück zum Einpassen in den verfügbaren Platz.
    pub fn zoom_fit(&self) {
        self.imp().viewport.borrow_mut().set_fit();
        self.imp().changed();
    }

    /// 100 %: ein Bildpixel je Bildschirmpixel, um die Mitte.
    pub fn zoom_original(&self) {
        let imp = self.imp();
        let center = imp.center();
        imp.viewport.borrow_mut().set_zoom_at(center, 1.0);
        imp.changed();
    }

    /// `callback` erhält den Zoomfaktor (1.0 = 100 %), auch wenn er sich durch
    /// eine neue Fenstergröße ändert.
    pub fn connect_zoom_changed(&self, callback: impl Fn(f64) + 'static) {
        self.imp().on_zoom_changed.replace(Some(Box::new(callback)));
    }
}
