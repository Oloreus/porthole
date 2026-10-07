use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use crate::annotations::{Color, Shape, Tool};

mod imp {
    use std::cell::{Cell, RefCell};

    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{gdk, glib, graphene, gsk};

    use crate::annotations::{self, Annotations, Color, Tool};
    use crate::viewport::{Viewport, ZoomMode, ZOOM_STEP};

    /// Pan-Weg je Mausrad-Raste.
    const WHEEL_PAN_STEP: f64 = 48.0;
    /// Touchpads liefern Pixel statt Rasten; so viele entsprechen einem
    /// Zoomschritt.
    const TOUCHPAD_PIXELS_PER_STEP: f64 = 50.0;

    type ZoomCallback = Box<dyn Fn(f64)>;
    type AnnotationsCallback = Box<dyn Fn(bool)>;

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
        pub tool: Cell<Tool>,
        pub color: Cell<Color>,
        pub annotations: RefCell<Annotations>,
        /// Receives whether any shapes exist, after each added or removed one.
        pub on_annotations_changed: RefCell<Option<AnnotationsCallback>>,
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

            // Shapes live in image pixels: map them onto the drawn image.
            let shapes = self.annotations.borrow().all();
            if !shapes.is_empty() {
                let pixels_to_widget = (zoom / scale) as f32;
                snapshot.push_clip(&bounds);
                snapshot.save();
                snapshot.translate(&graphene::Point::new(bounds.x(), bounds.y()));
                snapshot.scale(pixels_to_widget, pixels_to_widget);
                for node in annotations::render_nodes(&shapes) {
                    snapshot.append_node(node);
                }
                snapshot.restore();
                snapshot.pop();
            }
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

            // Primary button: draw with a shape tool, pan with the pointer.
            let drag = gtk::GestureDrag::builder().button(gdk::BUTTON_PRIMARY).build();
            drag.connect_drag_begin(glib::clone!(
                #[weak]
                obj,
                move |_, x, y| {
                    let imp = obj.imp();
                    match imp.tool.get() {
                        Tool::Shape(kind) => imp.begin_shape(kind, (x, y)),
                        Tool::Pointer => imp.begin_pan(),
                    }
                }
            ));
            drag.connect_drag_update(glib::clone!(
                #[weak]
                obj,
                move |drag, offset_x, offset_y| {
                    let imp = obj.imp();
                    if imp.annotations.borrow().is_drawing() {
                        if let Some((x, y)) = drag.start_point() {
                            imp.update_shape((x + offset_x, y + offset_y));
                        }
                    } else {
                        imp.update_pan(offset_x, offset_y);
                    }
                }
            ));
            drag.connect_drag_end(glib::clone!(
                #[weak]
                obj,
                move |_, _, _| {
                    let imp = obj.imp();
                    imp.finish_shape(true);
                    imp.end_drag();
                }
            ));
            drag.connect_cancel(glib::clone!(
                #[weak]
                obj,
                move |_, _| {
                    let imp = obj.imp();
                    imp.finish_shape(false);
                    imp.end_drag();
                }
            ));
            obj.add_controller(drag);

            // Middle button: always pans, also while a shape tool is active.
            let pan = gtk::GestureDrag::builder().button(gdk::BUTTON_MIDDLE).build();
            pan.connect_drag_begin(glib::clone!(
                #[weak]
                obj,
                move |_, _, _| obj.imp().begin_pan()
            ));
            pan.connect_drag_update(glib::clone!(
                #[weak]
                obj,
                move |_, offset_x, offset_y| obj.imp().update_pan(offset_x, offset_y)
            ));
            pan.connect_drag_end(glib::clone!(
                #[weak]
                obj,
                move |_, _, _| obj.imp().end_drag()
            ));
            pan.connect_cancel(glib::clone!(
                #[weak]
                obj,
                move |_, _| obj.imp().end_drag()
            ));
            obj.add_controller(pan);

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

        fn begin_pan(&self) {
            let pan = {
                let viewport = self.viewport.borrow();
                viewport.is_pannable().then(|| viewport.pan())
            };
            if pan.is_some() {
                self.drag_start_pan.set(pan);
                self.update_cursor();
            }
        }

        fn update_pan(&self, offset_x: f64, offset_y: f64) {
            if let Some((x, y)) = self.drag_start_pan.get() {
                self.viewport.borrow_mut().pan_to(x + offset_x, y + offset_y);
                self.changed();
            }
        }

        fn begin_shape(&self, kind: annotations::ShapeKind, point: (f64, f64)) {
            let Some((size, point)) = self.pixel_point(point) else {
                return;
            };
            let width = annotations::default_stroke_width(size.0, size.1);
            self.annotations
                .borrow_mut()
                .begin(kind, self.color.get(), width, point);
            self.obj().queue_draw();
        }

        fn update_shape(&self, point: (f64, f64)) {
            if let Some((_, point)) = self.pixel_point(point) {
                self.annotations.borrow_mut().update(point);
                self.obj().queue_draw();
            }
        }

        /// Ends a shape drag: keeps the shape on `commit`, drops it otherwise.
        fn finish_shape(&self, commit: bool) {
            let added = {
                let mut annotations = self.annotations.borrow_mut();
                if !annotations.is_drawing() {
                    return;
                }
                if commit {
                    annotations.commit()
                } else {
                    annotations.cancel();
                    false
                }
            };
            self.obj().queue_draw();
            if added {
                self.notify_annotations();
            }
        }

        pub fn notify_annotations(&self) {
            let has_shapes = !self.annotations.borrow().is_empty();
            if let Some(callback) = self.on_annotations_changed.borrow().as_ref() {
                callback(has_shapes);
            }
        }

        /// Widget point → image pixel (clamped to the image), plus the image
        /// size in pixels.
        fn pixel_point(&self, point: (f64, f64)) -> Option<((i32, i32), (f64, f64))> {
            let texture = self.texture.borrow();
            let texture = texture.as_ref()?;
            let size = (texture.width(), texture.height());
            let (x, y) = self.viewport.borrow().image_point(point);
            let scale = self.surface_scale();
            Some((size, annotations::clamp_to_image((x * scale, y * scale), size)))
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

        pub fn update_cursor(&self) {
            let cursor = if self.drag_start_pan.get().is_some() {
                Some("grabbing")
            } else if self.tool.get() != Tool::Pointer {
                Some("crosshair")
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

    pub fn set_tool(&self, tool: Tool) {
        let imp = self.imp();
        imp.tool.set(tool);
        imp.update_cursor();
    }

    /// Color for shapes drawn from now on; existing ones keep theirs.
    pub fn set_color(&self, color: Color) {
        self.imp().color.set(color);
    }

    /// Removes the most recent shape.
    pub fn undo_annotation(&self) {
        let imp = self.imp();
        if imp.annotations.borrow_mut().undo() {
            self.queue_draw();
            imp.notify_annotations();
        }
    }

    /// The committed shapes and their revision (changes with every edit).
    pub fn annotations(&self) -> (Vec<Shape>, u64) {
        let annotations = self.imp().annotations.borrow();
        (annotations.shapes().to_vec(), annotations.revision())
    }

    /// `callback` receives whether any shapes exist, after each change.
    pub fn connect_annotations_changed(&self, callback: impl Fn(bool) + 'static) {
        self.imp().on_annotations_changed.replace(Some(Box::new(callback)));
    }

    /// `callback` erhält den Zoomfaktor (1.0 = 100 %), auch wenn er sich durch
    /// eine neue Fenstergröße ändert.
    pub fn connect_zoom_changed(&self, callback: impl Fn(f64) + 'static) {
        self.imp().on_zoom_changed.replace(Some(Box::new(callback)));
    }
}
