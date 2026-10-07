use gtk::glib;
use gtk::subclass::prelude::*;

use crate::annotations::Tool;

const SIZE: i32 = 16;

mod imp {
    use std::cell::Cell;

    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{glib, gsk};

    use super::SIZE;
    use crate::annotations::{self, Color, Shape, ShapeKind, Tool};

    /// Outline of a mouse pointer, in icon coordinates.
    const POINTER: [(f32, f32); 7] = [
        (3.0, 1.0),
        (3.0, 14.0),
        (6.5, 10.5),
        (9.0, 15.0),
        (11.0, 14.0),
        (8.5, 9.5),
        (13.0, 9.5),
    ];

    #[derive(Default)]
    pub struct ToolIcon {
        pub tool: Cell<Tool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ToolIcon {
        const NAME: &'static str = "PortholeToolIcon";
        type Type = super::ToolIcon;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ToolIcon {}

    impl WidgetImpl for ToolIcon {
        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (SIZE, SIZE, -1, -1)
        }

        // The foreground color can change with the state (e.g. insensitive).
        fn state_flags_changed(&self, previous: &gtk::StateFlags) {
            self.parent_state_flags_changed(previous);
            self.obj().queue_draw();
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let color = obj.color();
            let kind = match self.tool.get() {
                Tool::Pointer => {
                    let builder = gsk::PathBuilder::new();
                    builder.move_to(POINTER[0].0, POINTER[0].1);
                    for (x, y) in &POINTER[1..] {
                        builder.line_to(*x, *y);
                    }
                    builder.close();
                    snapshot.append_fill(&builder.to_path(), gsk::FillRule::Winding, &color);
                    return;
                }
                Tool::Shape(kind) => kind,
            };
            // Same geometry as the shapes on the screenshot.
            let (from, to) = match kind {
                ShapeKind::Rectangle => ((2.0, 3.0), (14.0, 13.0)),
                ShapeKind::Ellipse => ((2.0, 3.0), (14.0, 13.0)),
                ShapeKind::Arrow => ((2.0, 14.0), (14.0, 2.0)),
            };
            let shape = Shape {
                kind,
                from,
                to,
                color: Color::default(),
                // 2 px on whole coordinates: crisp edges, like Adwaita's symbolic icons.
                width: 2.0,
            };
            snapshot.append_stroke(
                &annotations::path(&shape),
                &annotations::stroke(shape.width),
                &color,
            );
        }
    }
}

glib::wrapper! {
    /// Symbolic 16 px icon for a drawing tool, in the current foreground
    /// color. Drawn in code: the icon theme has no shape icons, and gdk4-rs
    /// can't implement `GtkSymbolicPaintable` yet.
    pub struct ToolIcon(ObjectSubclass<imp::ToolIcon>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ToolIcon {
    pub fn new(tool: Tool) -> Self {
        // The button allocates more than 16 px; the drawing assumes exactly
        // that size, so keep the widget at it and centered.
        let icon: Self = glib::Object::builder()
            .property("halign", gtk::Align::Center)
            .property("valign", gtk::Align::Center)
            .build();
        icon.imp().tool.set(tool);
        icon
    }
}
