//! Shapes drawn on top of a screenshot (rectangle, ellipse, arrow).
//!
//! The model is kept in image pixels, independent of zoom and pan. The same
//! render nodes are used for the preview (under the view transformation) and
//! for the exported image, so what you see is exactly what gets copied/saved.
//! Only `gsk`/`gdk` types are used here – no GTK initialization needed, which
//! keeps it testable without a display.

use gtk::prelude::*;
use gtk::{gdk, graphene, gsk};

/// Drags shorter than this (in image pixels) are treated as clicks.
const MIN_EXTENT: f64 = 3.0;
/// Angle between the arrow shaft and each side of its head.
const ARROW_HEAD_ANGLE: f64 = std::f64::consts::PI / 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Arrow,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// Pan and zoom, no drawing.
    #[default]
    Pointer,
    Shape(ShapeKind),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Color {
    #[default]
    Red,
    Orange,
    Green,
    Blue,
    White,
    Black,
}

impl Color {
    pub const ALL: [Color; 6] = [
        Color::Red,
        Color::Orange,
        Color::Green,
        Color::Blue,
        Color::White,
        Color::Black,
    ];

    /// Stable identifier for the settings file and CSS class names.
    pub fn name(self) -> &'static str {
        match self {
            Color::Red => "red",
            Color::Orange => "orange",
            Color::Green => "green",
            Color::Blue => "blue",
            Color::White => "white",
            Color::Black => "black",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|color| color.name() == name)
    }

    /// GNOME palette colors, as `#rrggbb`.
    pub fn hex(self) -> &'static str {
        match self {
            Color::Red => "#e01b24",
            Color::Orange => "#ff7800",
            Color::Green => "#2ec27e",
            Color::Blue => "#3584e4",
            Color::White => "#ffffff",
            Color::Black => "#000000",
        }
    }

    pub fn rgba(self) -> gdk::RGBA {
        let hex = &self.hex()[1..];
        let channel =
            |i: usize| f32::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0)) / 255.0;
        gdk::RGBA::new(channel(0), channel(2), channel(4), 1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub kind: ShapeKind,
    /// Where the drag started, in image pixels.
    pub from: (f64, f64),
    /// Where the drag currently ends, in image pixels.
    pub to: (f64, f64),
    pub color: Color,
    /// Stroke width in image pixels.
    pub width: f64,
}

impl Shape {
    /// Too small to be intentional (e.g. a click or double-click).
    fn is_degenerate(&self) -> bool {
        let (dx, dy) = (
            (self.to.0 - self.from.0).abs(),
            (self.to.1 - self.from.1).abs(),
        );
        match self.kind {
            ShapeKind::Arrow => dx.hypot(dy) < MIN_EXTENT,
            ShapeKind::Rectangle | ShapeKind::Ellipse => dx < MIN_EXTENT && dy < MIN_EXTENT,
        }
    }

    /// Bounding box of the dragged area, independent of the drag direction.
    pub fn bounds(&self) -> graphene::Rect {
        let x = self.from.0.min(self.to.0);
        let y = self.from.1.min(self.to.1);
        graphene::Rect::new(
            x as f32,
            y as f32,
            (self.from.0 - self.to.0).abs() as f32,
            (self.from.1 - self.to.1).abs() as f32,
        )
    }
}

/// The shapes of one screenshot plus the one being dragged right now.
#[derive(Debug, Default)]
pub struct Annotations {
    shapes: Vec<Shape>,
    draft: Option<Shape>,
    /// Increases with every change of the committed shapes; lets callers
    /// cache an exported image.
    revision: u64,
}

impl Annotations {
    pub fn begin(&mut self, kind: ShapeKind, color: Color, width: f64, point: (f64, f64)) {
        self.draft = Some(Shape {
            kind,
            from: point,
            to: point,
            color,
            width,
        });
    }

    pub fn update(&mut self, point: (f64, f64)) {
        if let Some(draft) = &mut self.draft {
            draft.to = point;
        }
    }

    /// Finishes the drag. Returns `true` if a shape was added.
    pub fn commit(&mut self) -> bool {
        match self.draft.take() {
            Some(draft) if !draft.is_degenerate() => {
                self.shapes.push(draft);
                self.revision += 1;
                true
            }
            _ => false,
        }
    }

    pub fn cancel(&mut self) {
        self.draft = None;
    }

    pub fn is_drawing(&self) -> bool {
        self.draft.is_some()
    }

    /// Removes the most recent shape. Returns `false` if there was none.
    pub fn undo(&mut self) -> bool {
        let removed = self.shapes.pop().is_some();
        if removed {
            self.revision += 1;
        }
        removed
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    pub fn shapes(&self) -> &[Shape] {
        &self.shapes
    }

    /// Committed shapes plus the draft, in drawing order.
    pub fn all(&self) -> Vec<Shape> {
        self.shapes.iter().copied().chain(self.draft).collect()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

/// Stroke width that stays visible on large screenshots without covering
/// small ones.
pub fn default_stroke_width(width: i32, height: i32) -> f64 {
    (f64::from(width.min(height)) / 200.0).clamp(2.0, 6.0)
}

pub fn clamp_to_image(point: (f64, f64), (width, height): (i32, i32)) -> (f64, f64) {
    (
        point.0.clamp(0.0, f64::from(width)),
        point.1.clamp(0.0, f64::from(height)),
    )
}

/// Outline of a shape, in image pixels.
pub fn path(shape: &Shape) -> gsk::Path {
    let builder = gsk::PathBuilder::new();
    let bounds = shape.bounds();
    match shape.kind {
        ShapeKind::Rectangle => builder.add_rect(&bounds),
        ShapeKind::Ellipse => {
            // Corner radii of half the size turn the rounded rect into an
            // exact ellipse.
            let radius = graphene::Size::new(bounds.width() / 2.0, bounds.height() / 2.0);
            builder.add_rounded_rect(&gsk::RoundedRect::new(
                bounds, radius, radius, radius, radius,
            ));
        }
        ShapeKind::Arrow => {
            let (from, to) = (shape.from, shape.to);
            builder.move_to(from.0 as f32, from.1 as f32);
            builder.line_to(to.0 as f32, to.1 as f32);
            if let Some((left, right)) = arrow_head(shape) {
                builder.move_to(left.0 as f32, left.1 as f32);
                builder.line_to(to.0 as f32, to.1 as f32);
                builder.line_to(right.0 as f32, right.1 as f32);
            }
        }
    }
    builder.to_path()
}

/// The two outer points of the arrow head; `None` for a zero-length arrow.
fn arrow_head(shape: &Shape) -> Option<((f64, f64), (f64, f64))> {
    let (dx, dy) = (shape.to.0 - shape.from.0, shape.to.1 - shape.from.1);
    let length = dx.hypot(dy);
    if length == 0.0 {
        return None;
    }
    // Short arrows get a smaller head, so it never swallows the shaft.
    let head = (shape.width * 4.0).min(length * 0.6);
    let angle = dy.atan2(dx);
    let point = |offset: f64| {
        let side = angle + std::f64::consts::PI + offset;
        (
            shape.to.0 + head * side.cos(),
            shape.to.1 + head * side.sin(),
        )
    };
    Some((point(-ARROW_HEAD_ANGLE), point(ARROW_HEAD_ANGLE)))
}

pub fn stroke(width: f64) -> gsk::Stroke {
    let stroke = gsk::Stroke::new(width as f32);
    stroke.set_line_cap(gsk::LineCap::Round);
    stroke.set_line_join(gsk::LineJoin::Round);
    stroke
}

/// GSK registers its render node types during GTK initialization; touching
/// the GTypes does it as well. Only matters without GTK (unit tests).
pub fn register_node_types() {
    for node_type in [
        gsk::TextureNode::static_type(),
        gsk::ContainerNode::static_type(),
        gsk::ColorNode::static_type(),
        gsk::StrokeNode::static_type(),
    ] {
        debug_assert!(node_type.is_valid());
    }
}

/// One render node per shape, in image pixel coordinates.
pub fn render_nodes(shapes: &[Shape]) -> Vec<gsk::RenderNode> {
    register_node_types();
    shapes
        .iter()
        .filter_map(|shape| {
            let path = path(shape);
            let stroke = stroke(shape.width);
            let bounds = path.stroke_bounds(&stroke)?;
            let fill = gsk::ColorNode::new(&shape.color.rgba(), &bounds);
            Some(gsk::StrokeNode::new(fill, &path, &stroke).upcast())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(
        annotations: &mut Annotations,
        kind: ShapeKind,
        from: (f64, f64),
        to: (f64, f64),
    ) -> bool {
        annotations.begin(kind, Color::Red, 4.0, from);
        annotations.update(to);
        annotations.commit()
    }

    #[test]
    fn bounds_are_normalized_for_any_drag_direction() {
        let shape = |from, to| Shape {
            kind: ShapeKind::Rectangle,
            from,
            to,
            color: Color::Red,
            width: 2.0,
        };
        let expected = graphene::Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(shape((10.0, 20.0), (40.0, 60.0)).bounds(), expected);
        assert_eq!(shape((40.0, 60.0), (10.0, 20.0)).bounds(), expected);
        assert_eq!(shape((40.0, 20.0), (10.0, 60.0)).bounds(), expected);
    }

    #[test]
    fn clicks_do_not_create_shapes() {
        let mut annotations = Annotations::default();
        assert!(!drag(
            &mut annotations,
            ShapeKind::Rectangle,
            (5.0, 5.0),
            (6.0, 7.0)
        ));
        assert!(!drag(
            &mut annotations,
            ShapeKind::Arrow,
            (5.0, 5.0),
            (6.0, 6.0)
        ));
        assert!(annotations.is_empty());
        assert_eq!(annotations.revision(), 0);
        // A thin but long rectangle is intentional.
        assert!(drag(
            &mut annotations,
            ShapeKind::Rectangle,
            (5.0, 5.0),
            (100.0, 6.0)
        ));
    }

    #[test]
    fn draft_is_visible_but_not_committed() {
        let mut annotations = Annotations::default();
        annotations.begin(ShapeKind::Ellipse, Color::Blue, 3.0, (0.0, 0.0));
        annotations.update((50.0, 50.0));
        assert!(annotations.is_drawing());
        assert_eq!(annotations.all().len(), 1);
        assert!(annotations.is_empty());
        annotations.cancel();
        assert!(annotations.all().is_empty());
    }

    #[test]
    fn undo_removes_newest_first_and_bumps_revision() {
        let mut annotations = Annotations::default();
        drag(
            &mut annotations,
            ShapeKind::Rectangle,
            (0.0, 0.0),
            (10.0, 10.0),
        );
        drag(&mut annotations, ShapeKind::Arrow, (0.0, 0.0), (20.0, 20.0));
        assert_eq!(annotations.revision(), 2);

        assert!(annotations.undo());
        assert_eq!(annotations.shapes().len(), 1);
        assert_eq!(annotations.shapes()[0].kind, ShapeKind::Rectangle);
        assert_eq!(annotations.revision(), 3);

        assert!(annotations.undo());
        assert!(!annotations.undo());
        assert_eq!(annotations.revision(), 4);
    }

    #[test]
    fn stroke_width_follows_image_size_within_limits() {
        assert_eq!(default_stroke_width(100, 80), 2.0);
        assert_eq!(default_stroke_width(1000, 800), 4.0);
        assert_eq!(default_stroke_width(8000, 6000), 6.0);
    }

    #[test]
    fn points_are_clamped_to_the_image() {
        assert_eq!(clamp_to_image((-5.0, 120.0), (100, 100)), (0.0, 100.0));
        assert_eq!(clamp_to_image((50.0, 50.0), (100, 100)), (50.0, 50.0));
    }

    #[test]
    fn color_names_roundtrip() {
        for color in Color::ALL {
            assert_eq!(Color::from_name(color.name()), Some(color));
        }
        assert_eq!(Color::from_name("purple"), None);
        assert_eq!(
            Color::Red.rgba(),
            gdk::RGBA::new(224.0 / 255.0, 27.0 / 255.0, 36.0 / 255.0, 1.0)
        );
    }

    #[test]
    fn arrow_head_points_back_along_the_shaft() {
        let shape = Shape {
            kind: ShapeKind::Arrow,
            from: (0.0, 0.0),
            to: (100.0, 0.0),
            color: Color::Red,
            width: 4.0,
        };
        let (left, right) = arrow_head(&shape).unwrap();
        assert!(left.0 < 100.0 && right.0 < 100.0);
        assert!((left.1 + right.1).abs() < 1e-9, "head must be symmetric");
        assert_eq!(render_nodes(&[shape]).len(), 1);
    }
}
