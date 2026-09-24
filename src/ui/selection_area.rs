use gtk::glib;
use gtk::subclass::prelude::*;

use crate::geometry::Rect;

mod imp {
    use std::cell::{Cell, RefCell};

    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{gdk, glib, graphene, gsk};

    use crate::geometry::Rect;

    const DIM: gdk::RGBA = gdk::RGBA::new(0.0, 0.0, 0.0, 0.45);
    const BORDER_INNER: gdk::RGBA = gdk::RGBA::new(1.0, 1.0, 1.0, 1.0);
    const BORDER_OUTER: gdk::RGBA = gdk::RGBA::new(0.0, 0.0, 0.0, 0.7);
    const LABEL_BG: gdk::RGBA = gdk::RGBA::new(0.0, 0.0, 0.0, 0.75);
    const LABEL_FG: gdk::RGBA = gdk::RGBA::new(1.0, 1.0, 1.0, 1.0);
    const HINT: &str = "Ziehen zum Aufnehmen  ·  ESC bricht ab";

    #[derive(Default)]
    pub struct SelectionArea {
        pub texture: RefCell<Option<gdk::Texture>>,
        /// This monitor's part of the selection, in widget coordinates.
        pub selection: Cell<Option<Rect>>,
        /// Size of the whole selection; only one monitor shows it.
        pub label: RefCell<Option<String>>,
        pub show_hint: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SelectionArea {
        const NAME: &'static str = "PortholeSelectionArea";
        type Type = super::SelectionArea;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for SelectionArea {}

    impl WidgetImpl for SelectionArea {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let (width, height) = (widget.width() as f32, widget.height() as f32);
            let bounds = graphene::Rect::new(0.0, 0.0, width, height);

            if let Some(texture) = self.texture.borrow().as_ref() {
                snapshot.append_scaled_texture(texture, gsk::ScalingFilter::Linear, &bounds);
            }

            let Some(selection) = self.selection.get() else {
                snapshot.append_color(&DIM, &bounds);
                if self.show_hint.get() {
                    self.draw_text(snapshot, HINT, width / 2.0, height / 2.0, true);
                }
                return;
            };

            // Dimming as four rectangles around the selection.
            let (x, y) = (selection.x as f32, selection.y as f32);
            let (w, h) = (selection.width as f32, selection.height as f32);
            for dim in [
                graphene::Rect::new(0.0, 0.0, width, y),
                graphene::Rect::new(0.0, y + h, width, height - y - h),
                graphene::Rect::new(0.0, y, x, h),
                graphene::Rect::new(x + w, y, width - x - w, h),
            ] {
                snapshot.append_color(&DIM, &dim);
            }

            // Light border with a dark outline: visible on any background.
            // Both lie outside the selection and cover no content.
            for (inset, color) in [(-2.0, BORDER_OUTER), (-1.0, BORDER_INNER)] {
                let rect = graphene::Rect::new(x, y, w, h).inset_r(inset, inset);
                snapshot.append_border(
                    &gsk::RoundedRect::from_rect(rect, 0.0),
                    &[1.0; 4],
                    &[color; 4],
                );
            }

            if let Some(text) = self.label.borrow().as_deref() {
                // Below the selection; above it at the bottom screen edge.
                let below = y + h + 8.0;
                let label_y = if below + 28.0 < height { below } else { y - 32.0 };
                self.draw_text(snapshot, text, x.max(4.0), label_y.max(4.0), false);
            }
        }
    }

    impl SelectionArea {
        fn draw_text(&self, snapshot: &gtk::Snapshot, text: &str, x: f32, y: f32, centered: bool) {
            let layout = self.obj().create_pango_layout(Some(text));
            let (text_width, text_height) = layout.pixel_size();
            let (text_width, text_height) = (text_width as f32, text_height as f32);
            let (pad_x, pad_y) = (8.0, 4.0);

            let (x, y) = if centered {
                (x - text_width / 2.0 - pad_x, y - text_height / 2.0 - pad_y)
            } else {
                (x, y)
            };

            snapshot.append_color(
                &LABEL_BG,
                &graphene::Rect::new(x, y, text_width + 2.0 * pad_x, text_height + 2.0 * pad_y),
            );
            snapshot.save();
            snapshot.translate(&graphene::Point::new(x + pad_x, y + pad_y));
            snapshot.append_layout(&layout, &LABEL_FG);
            snapshot.restore();
        }
    }
}

glib::wrapper! {
    /// Draws one monitor's frozen desktop, the dimming and the selection.
    /// Display only – input is handled by the overlay session.
    pub struct SelectionArea(ObjectSubclass<imp::SelectionArea>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SelectionArea {
    pub fn new(texture: &gtk::gdk::Texture) -> Self {
        let area: Self = glib::Object::new();
        let imp = area.imp();
        imp.texture.replace(Some(texture.clone()));
        imp.show_hint.set(true);
        area
    }

    /// `selection`: this monitor's part of the selection (widget coordinates),
    /// `None` if the selection doesn't touch it. Hides the hint – as soon as
    /// a drag happens anywhere, on all monitors.
    pub fn set_selection(&self, selection: Option<Rect>, label: Option<String>) {
        use gtk::prelude::WidgetExt;
        let imp = self.imp();
        imp.selection.set(selection);
        imp.label.replace(label);
        imp.show_hint.set(false);
        self.queue_draw();
    }
}
