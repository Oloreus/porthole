//! Annotations drawn on top of a screenshot: shapes (rectangle, ellipse,
//! arrow) and text.
//!
//! The model is kept in image pixels, independent of zoom and pan. The same
//! render nodes are used for the preview (under the view transformation) and
//! for the exported image, so what you see is exactly what gets copied/saved.
//! Only `gsk`/`gdk`/`pango` types are used here – no GTK initialization
//! needed, which keeps it testable without a display.

use std::cell::OnceCell;

use gtk::prelude::*;
use gtk::{gdk, graphene, gsk, pango};

/// Drags shorter than this (in image pixels) are treated as clicks.
const MIN_EXTENT: f64 = 3.0;
/// Angle between the arrow shaft and each side of its head.
const ARROW_HEAD_ANGLE: f64 = std::f64::consts::PI / 6.0;
/// Font of the text annotations; the size comes from the image.
const FONT: &str = "Sans Bold";
/// Outline stroke width per font size. Half of it lies under the glyph fill.
const OUTLINE_RATIO: f64 = 1.0 / 10.0;
/// Minimum outline stroke width in image pixels (about one pixel visible).
const MIN_OUTLINE: f64 = 2.0;
/// Caret width per font size.
const CARET_RATIO: f64 = 1.0 / 12.0;

thread_local! {
    /// Pango context for laying out text, independent of any widget.
    static TEXT_CONTEXT: OnceCell<pango::Context> = const { OnceCell::new() };
}

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
    /// Click on the image, then type.
    Text,
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

    /// Contrasting outline around text, so it stays readable on any
    /// background.
    pub fn outline(self) -> Color {
        match self {
            Color::Black => Color::White,
            _ => Color::Black,
        }
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

#[derive(Clone, Debug, PartialEq)]
pub struct TextItem {
    /// Top left corner of the text, in image pixels.
    pub origin: (f64, f64),
    /// May contain line breaks.
    pub text: String,
    pub color: Color,
    /// Font size in image pixels.
    pub size: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Annotation {
    Shape(Shape),
    Text(TextItem),
}

/// A committed text taken out for editing.
#[derive(Debug)]
struct Edit {
    /// Its place in the drawing order.
    index: usize,
    original: TextItem,
    /// Committed annotations before the edit, for undo.
    before: Vec<Annotation>,
}

/// A committed text being dragged to another place.
#[derive(Debug)]
struct Move {
    index: usize,
    start_origin: (f64, f64),
    /// Committed annotations before the move, for undo.
    before: Vec<Annotation>,
}

/// The annotations of one screenshot plus the shape being dragged or the
/// text being typed right now.
#[derive(Debug, Default)]
pub struct Annotations {
    items: Vec<Annotation>,
    draft: Option<Shape>,
    text_draft: Option<TextItem>,
    /// Caret position in the text being typed: byte index on a char boundary.
    cursor: usize,
    editing: Option<Edit>,
    moving: Option<Move>,
    /// Committed annotations before each change, newest last.
    history: Vec<Vec<Annotation>>,
    /// Increases with every change of the committed annotations; lets callers
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
                self.record(self.items.clone());
                self.items.push(Annotation::Shape(draft));
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

    /// Starts typing a text whose first line is vertically centered on
    /// `point`.
    pub fn begin_text(&mut self, color: Color, size: f64, point: (f64, f64)) {
        let mut text = TextItem {
            origin: point,
            text: String::new(),
            color,
            size,
        };
        text.origin.1 -= caret(&text, 0).height() as f64 / 2.0;
        self.text_draft = Some(text);
        self.cursor = 0;
        self.editing = None;
    }

    /// Topmost committed text at `point` (image pixels), as an index into
    /// [`Self::items`].
    pub fn text_at(&self, point: (f64, f64)) -> Option<usize> {
        self.items.iter().rposition(|item| match item {
            Annotation::Text(text) => text_bounds(text).contains_point(&graphene::Point::new(
                point.0 as f32,
                point.1 as f32,
            )),
            Annotation::Shape(_) => false,
        })
    }

    /// Takes the committed text at `index` out for editing, caret at its
    /// end. Committing puts it back at the same place in the drawing order.
    pub fn begin_edit(&mut self, index: usize) {
        if !matches!(self.items.get(index), Some(Annotation::Text(_))) {
            return;
        }
        let before = self.items.clone();
        let Annotation::Text(text) = self.items.remove(index) else {
            unreachable!();
        };
        self.cursor = text.text.len();
        self.text_draft = Some(text.clone());
        self.editing = Some(Edit {
            index,
            original: text,
            before,
        });
    }

    /// Starts dragging the committed text at `index`.
    pub fn begin_move(&mut self, index: usize) {
        if let Some(Annotation::Text(text)) = self.items.get(index) {
            self.moving = Some(Move {
                index,
                start_origin: text.origin,
                before: self.items.clone(),
            });
        }
    }

    /// Moves the dragged text by `delta` (image pixels) from where the drag
    /// started.
    pub fn update_move(&mut self, delta: (f64, f64)) {
        let Some(moving) = &self.moving else {
            return;
        };
        if let Some(Annotation::Text(text)) = self.items.get_mut(moving.index) {
            text.origin = (
                moving.start_origin.0 + delta.0,
                moving.start_origin.1 + delta.1,
            );
        }
    }

    /// Finishes the drag. Returns `true` if the text ended up elsewhere.
    pub fn commit_move(&mut self) -> bool {
        let Some(moving) = self.moving.take() else {
            return false;
        };
        let moved = matches!(
            self.items.get(moving.index),
            Some(Annotation::Text(text)) if text.origin != moving.start_origin
        );
        if moved {
            self.record(moving.before);
        }
        moved
    }

    /// Puts the dragged text back where it was.
    pub fn cancel_move(&mut self) {
        if let Some(moving) = self.moving.take() {
            if let Some(Annotation::Text(text)) = self.items.get_mut(moving.index) {
                text.origin = moving.start_origin;
            }
        }
    }

    pub fn is_moving(&self) -> bool {
        self.moving.is_some()
    }

    /// Removes the committed text at `index`. Returns `false` if there is
    /// no text.
    pub fn remove_text(&mut self, index: usize) -> bool {
        if !matches!(self.items.get(index), Some(Annotation::Text(_))) {
            return false;
        }
        self.record(self.items.clone());
        self.items.remove(index);
        true
    }

    /// Drops the text being typed; an edited text is removed for good.
    /// Returns `true` if the committed annotations changed.
    pub fn remove_edited_text(&mut self) -> bool {
        self.text_draft = None;
        match self.editing.take() {
            Some(edit) => {
                self.record(edit.before);
                true
            }
            None => false,
        }
    }

    /// Inserts at the caret and moves the caret behind the input.
    pub fn insert(&mut self, input: &str) {
        if let Some(draft) = &mut self.text_draft {
            draft.text.insert_str(self.cursor, input);
            self.cursor += input.len();
        }
    }

    /// Removes the character before the caret.
    pub fn backspace(&mut self) {
        if let Some(draft) = &mut self.text_draft {
            if let Some((index, _)) = draft.text[..self.cursor].char_indices().next_back() {
                draft.text.remove(index);
                self.cursor = index;
            }
        }
    }

    /// Removes the character after the caret.
    pub fn delete(&mut self) {
        if let Some(draft) = &mut self.text_draft {
            if self.cursor < draft.text.len() {
                draft.text.remove(self.cursor);
            }
        }
    }

    pub fn move_left(&mut self) {
        if let Some(draft) = &self.text_draft {
            if let Some((index, _)) = draft.text[..self.cursor].char_indices().next_back() {
                self.cursor = index;
            }
        }
    }

    pub fn move_right(&mut self) {
        if let Some(draft) = &self.text_draft {
            if let Some(c) = draft.text[self.cursor..].chars().next() {
                self.cursor += c.len_utf8();
            }
        }
    }

    /// Caret to the start of its line.
    pub fn move_home(&mut self) {
        if let Some(draft) = &self.text_draft {
            self.cursor = draft.text[..self.cursor].rfind('\n').map_or(0, |index| index + 1);
        }
    }

    /// Caret to the end of its line.
    pub fn move_end(&mut self) {
        if let Some(draft) = &self.text_draft {
            self.cursor += draft.text[self.cursor..]
                .find('\n')
                .unwrap_or(draft.text.len() - self.cursor);
        }
    }

    pub fn text_cursor(&self) -> usize {
        self.cursor
    }

    /// Color for the text being typed, e.g. after picking another one.
    pub fn set_text_color(&mut self, color: Color) {
        if let Some(draft) = &mut self.text_draft {
            draft.color = color;
        }
    }

    /// Finishes typing. Returns `true` if the committed annotations changed:
    /// a new text was added, or an edited one was changed or removed. Blank
    /// text is dropped.
    pub fn commit_text(&mut self) -> bool {
        let Some(draft) = self.text_draft.take() else {
            return false;
        };
        let blank = draft.text.trim().is_empty();
        match self.editing.take() {
            Some(edit) => {
                let changed = blank || draft != edit.original;
                if changed {
                    self.record(edit.before);
                }
                self.items.insert(
                    edit.index,
                    Annotation::Text(if changed { draft } else { edit.original }),
                );
                if blank {
                    self.items.remove(edit.index);
                }
                changed
            }
            None if blank => false,
            None => {
                self.record(self.items.clone());
                self.items.push(Annotation::Text(draft));
                true
            }
        }
    }

    /// Drops the text being typed; an edited text goes back unchanged.
    pub fn cancel_text(&mut self) {
        self.text_draft = None;
        if let Some(edit) = self.editing.take() {
            self.items.insert(edit.index, Annotation::Text(edit.original));
        }
    }

    pub fn text_draft(&self) -> Option<&TextItem> {
        self.text_draft.as_ref()
    }

    pub fn is_editing_text(&self) -> bool {
        self.text_draft.is_some()
    }

    /// Remembers `before` as the state to go back to and marks the committed
    /// annotations as changed. Call it right before (or after) the change.
    fn record(&mut self, before: Vec<Annotation>) {
        self.history.push(before);
        self.revision += 1;
    }

    /// Reverts the most recent change: added, edited, moved or removed
    /// annotation. Returns `false` if there was none.
    pub fn undo(&mut self) -> bool {
        match self.history.pop() {
            Some(before) => {
                self.items = before;
                self.revision += 1;
                true
            }
            None => false,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[Annotation] {
        &self.items
    }

    /// Committed annotations plus the drafts, in drawing order.
    pub fn all(&self) -> Vec<Annotation> {
        self.items
            .iter()
            .cloned()
            .chain(self.draft.map(Annotation::Shape))
            .chain(self.text_draft.clone().map(Annotation::Text))
            .collect()
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

/// Font size that matches the stroke width of the shapes.
pub fn default_font_size(width: i32, height: i32) -> f64 {
    (f64::from(width.min(height)) / 40.0).clamp(14.0, 48.0)
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

/// Lays out `text` relative to its origin. Hinting is off, so the glyphs are
/// placed the same at every zoom level and in the exported image.
fn layout(text: &TextItem) -> pango::Layout {
    let context = TEXT_CONTEXT.with(|context| {
        context
            .get_or_init(|| {
                let context = pangocairo::FontMap::default().create_context();
                let mut options = gtk::cairo::FontOptions::new()
                    .expect("cairo font options can be created");
                options.set_hint_style(gtk::cairo::HintStyle::None);
                options.set_hint_metrics(gtk::cairo::HintMetrics::Off);
                pangocairo::functions::context_set_font_options(&context, Some(&options));
                context.set_round_glyph_positions(false);
                context
            })
            .clone()
    });
    let layout = pango::Layout::new(&context);
    let mut font = pango::FontDescription::from_string(FONT);
    font.set_absolute_size(text.size * f64::from(pango::SCALE));
    layout.set_font_description(Some(&font));
    layout.set_text(&text.text);
    layout
}

/// Glyph outlines of a text, relative to its origin.
fn text_path(text: &TextItem) -> gsk::Path {
    let builder = gsk::PathBuilder::new();
    builder.add_layout(&layout(text));
    builder.to_path()
}

/// Area a text covers including its outline, in image pixels.
fn text_bounds(text: &TextItem) -> graphene::Rect {
    let (_, logical) = layout(text).pixel_extents();
    let pad = outline_width(text) / 2.0;
    graphene::Rect::new(
        (text.origin.0 + f64::from(logical.x()) - pad) as f32,
        (text.origin.1 + f64::from(logical.y()) - pad) as f32,
        (f64::from(logical.width()) + 2.0 * pad) as f32,
        (f64::from(logical.height()) + 2.0 * pad) as f32,
    )
}

fn outline_width(text: &TextItem) -> f64 {
    (text.size * OUTLINE_RATIO).max(MIN_OUTLINE)
}

/// Caret before the byte index `cursor`, in image pixels.
pub fn caret(text: &TextItem, cursor: usize) -> graphene::Rect {
    let layout = layout(text);
    let (strong, _) = layout.cursor_pos(cursor.min(text.text.len()) as i32);
    let units = |value: i32| f64::from(value) / f64::from(pango::SCALE);
    let width = text.size * CARET_RATIO;
    graphene::Rect::new(
        (text.origin.0 + units(strong.x()) - width / 2.0) as f32,
        (text.origin.1 + units(strong.y())) as f32,
        width as f32,
        units(strong.height()) as f32,
    )
}

/// GSK registers its render node types during GTK initialization; touching
/// the GTypes does it as well. Only matters without GTK (unit tests).
pub fn register_node_types() {
    for node_type in [
        gsk::TextureNode::static_type(),
        gsk::ContainerNode::static_type(),
        gsk::ColorNode::static_type(),
        gsk::StrokeNode::static_type(),
        gsk::FillNode::static_type(),
        gsk::TransformNode::static_type(),
    ] {
        debug_assert!(node_type.is_valid());
    }
}

/// One render node per annotation, in image pixel coordinates.
pub fn render_nodes(annotations: &[Annotation]) -> Vec<gsk::RenderNode> {
    register_node_types();
    annotations
        .iter()
        .filter_map(|annotation| match annotation {
            Annotation::Shape(shape) => shape_node(shape),
            Annotation::Text(text) => text_node(text),
        })
        .collect()
}

fn shape_node(shape: &Shape) -> Option<gsk::RenderNode> {
    let path = path(shape);
    let stroke = stroke(shape.width);
    let bounds = path.stroke_bounds(&stroke)?;
    let fill = gsk::ColorNode::new(&shape.color.rgba(), &bounds);
    Some(gsk::StrokeNode::new(fill, &path, &stroke).upcast())
}

/// Glyphs in the text color over a contrasting outline.
fn text_node(text: &TextItem) -> Option<gsk::RenderNode> {
    let path = text_path(text);
    if path.is_empty() {
        return None;
    }
    let stroke = stroke(outline_width(text));
    let outline_color = gsk::ColorNode::new(
        &text.color.outline().rgba(),
        &path.stroke_bounds(&stroke)?,
    );
    let outline = gsk::StrokeNode::new(outline_color, &path, &stroke);
    let fill_color = gsk::ColorNode::new(&text.color.rgba(), &path.bounds()?);
    let fill = gsk::FillNode::new(fill_color, &path, gsk::FillRule::Winding);
    let glyphs = gsk::ContainerNode::new(&[outline.upcast(), fill.upcast()]);
    let origin = graphene::Point::new(text.origin.0 as f32, text.origin.1 as f32);
    Some(gsk::TransformNode::new(glyphs, &gsk::Transform::new().translate(&origin)).upcast())
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
        assert_eq!(annotations.items().len(), 1);
        assert!(matches!(
            annotations.items()[0],
            Annotation::Shape(Shape { kind: ShapeKind::Rectangle, .. })
        ));
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
        assert_eq!(render_nodes(&[Annotation::Shape(shape)]).len(), 1);
    }

    fn text(content: &str, size: f64) -> TextItem {
        TextItem {
            origin: (10.0, 20.0),
            text: content.to_string(),
            color: Color::Red,
            size,
        }
    }

    #[test]
    fn blank_text_is_dropped() {
        let mut annotations = Annotations::default();
        annotations.begin_text(Color::Red, 20.0, (10.0, 10.0));
        assert!(annotations.is_editing_text());
        annotations.insert(" \n ");
        assert!(!annotations.commit_text());
        assert!(!annotations.is_editing_text());
        assert!(annotations.is_empty());
        assert_eq!(annotations.revision(), 0);
    }

    #[test]
    fn typing_editing_and_undo() {
        let mut annotations = Annotations::default();
        annotations.begin_text(Color::Red, 20.0, (10.0, 10.0));
        annotations.insert("Grö");
        annotations.insert("ße");
        annotations.backspace();
        annotations.backspace();
        annotations.insert("\nä");
        annotations.set_text_color(Color::Blue);
        assert_eq!(annotations.all().len(), 1, "draft is visible");
        assert!(annotations.commit_text());
        let Annotation::Text(committed) = &annotations.items()[0] else {
            panic!("expected a text");
        };
        assert_eq!(committed.text, "Grö\nä");
        assert_eq!(committed.color, Color::Blue);
        assert_eq!(annotations.revision(), 1);
        assert!(annotations.undo());
        assert!(annotations.is_empty());
    }

    #[test]
    fn first_line_is_centered_on_the_click() {
        let mut annotations = Annotations::default();
        annotations.begin_text(Color::Red, 20.0, (10.0, 50.0));
        let caret = caret(annotations.text_draft().unwrap(), 0);
        let center = f64::from(caret.y() + caret.height() / 2.0);
        assert!((center - 50.0).abs() < 0.5, "caret center at {center}");
        assert!((f64::from(caret.x() + caret.width() / 2.0) - 10.0).abs() < 0.5);
    }

    #[test]
    fn text_renders_at_its_origin_and_grows_with_size() {
        let small = text_path(&text("Hi", 20.0)).bounds().unwrap();
        let large = text_path(&text("Hi", 40.0)).bounds().unwrap();
        assert!(large.width() > small.width() * 1.8);
        assert!(large.height() > small.height() * 1.8);
        // Relative to the origin: glyphs start near (0, 0).
        assert!(small.x() >= 0.0 && small.x() < 5.0);
        assert!(text_path(&text("", 20.0)).is_empty());

        let nodes = render_nodes(&[Annotation::Text(text("Hi", 20.0))]);
        assert_eq!(nodes.len(), 1);
        let bounds = nodes[0].bounds();
        assert!(bounds.x() >= 5.0 && bounds.y() >= 15.0, "translated to the origin");
        assert!(render_nodes(&[Annotation::Text(text("", 20.0))]).is_empty());
    }

    #[test]
    fn caret_moves_with_the_text() {
        let empty = caret(&text("", 20.0), 0);
        let typed = caret(&text("Hi", 20.0), 2);
        let two_lines = caret(&text("Hi\n", 20.0), 3);
        assert!(typed.x() > empty.x());
        assert!(two_lines.y() > typed.y());
        assert_eq!(two_lines.x(), empty.x());
        assert_eq!(caret(&text("Hi", 20.0), 0).x(), empty.x());
    }

    fn committed_text(annotations: &mut Annotations, content: &str, point: (f64, f64)) {
        annotations.begin_text(Color::Red, 20.0, point);
        annotations.insert(content);
        assert!(annotations.commit_text());
    }

    #[test]
    fn cursor_edits_in_the_middle() {
        let mut annotations = Annotations::default();
        annotations.begin_text(Color::Red, 20.0, (0.0, 0.0));
        annotations.insert("Grüße\nab");
        annotations.move_home();
        annotations.move_left();
        annotations.move_left();
        annotations.backspace();
        annotations.delete();
        annotations.insert("ß");
        assert_eq!(annotations.text_draft().unwrap().text, "Grüß\nab");
        annotations.move_end();
        annotations.insert("!");
        annotations.move_right();
        annotations.move_end();
        annotations.insert("c");
        assert_eq!(annotations.text_draft().unwrap().text, "Grüß!\nabc");
        annotations.move_home();
        annotations.delete();
        assert_eq!(annotations.text_draft().unwrap().text, "Grüß!\nbc");
    }

    #[test]
    fn texts_are_found_by_position() {
        let mut annotations = Annotations::default();
        drag(&mut annotations, ShapeKind::Rectangle, (0.0, 0.0), (300.0, 300.0));
        committed_text(&mut annotations, "Hello", (20.0, 50.0));
        assert_eq!(annotations.text_at((30.0, 50.0)), Some(1));
        assert_eq!(annotations.text_at((250.0, 250.0)), None);
        assert_eq!(annotations.text_at((10.0, 5.0)), None);
    }

    #[test]
    fn editing_keeps_the_drawing_order() {
        let mut annotations = Annotations::default();
        committed_text(&mut annotations, "One", (20.0, 50.0));
        drag(&mut annotations, ShapeKind::Arrow, (0.0, 0.0), (20.0, 20.0));
        let revision = annotations.revision();

        annotations.begin_edit(0);
        assert_eq!(annotations.items().len(), 1);
        assert_eq!(annotations.all().len(), 2, "edited text stays visible");
        annotations.insert("!");
        assert!(annotations.commit_text());
        assert_eq!(annotations.revision(), revision + 1);
        let Annotation::Text(edited) = &annotations.items()[0] else {
            panic!("expected the text at its old place");
        };
        assert_eq!(edited.text, "One!");

        // Unchanged: no new revision.
        annotations.begin_edit(0);
        assert!(!annotations.commit_text());
        assert_eq!(annotations.revision(), revision + 1);
    }

    #[test]
    fn cancelled_edit_restores_the_original() {
        let mut annotations = Annotations::default();
        committed_text(&mut annotations, "One", (20.0, 50.0));
        annotations.begin_edit(0);
        annotations.backspace();
        annotations.cancel_text();
        assert_eq!(annotations.items().len(), 1);
        assert!(matches!(&annotations.items()[0], Annotation::Text(t) if t.text == "One"));
        assert_eq!(annotations.revision(), 1);
    }

    #[test]
    fn emptied_text_is_removed() {
        let mut annotations = Annotations::default();
        committed_text(&mut annotations, "Hi", (20.0, 50.0));
        annotations.begin_edit(0);
        annotations.backspace();
        annotations.backspace();
        assert!(annotations.commit_text());
        assert!(annotations.is_empty());
        assert_eq!(annotations.revision(), 2);
    }

    fn origin(annotations: &Annotations, index: usize) -> (f64, f64) {
        match &annotations.items()[index] {
            Annotation::Text(text) => text.origin,
            Annotation::Shape(_) => panic!("expected a text"),
        }
    }

    #[test]
    fn moving_a_text_and_undoing_it() {
        let mut annotations = Annotations::default();
        committed_text(&mut annotations, "Hi", (20.0, 50.0));
        let start = origin(&annotations, 0);

        annotations.begin_move(0);
        assert!(annotations.is_moving());
        annotations.update_move((5.0, 5.0));
        annotations.update_move((30.0, -10.0));
        assert_eq!(annotations.revision(), 1, "no revision while dragging");
        assert!(annotations.commit_move());
        assert!(!annotations.is_moving());
        assert_eq!(origin(&annotations, 0), (start.0 + 30.0, start.1 - 10.0));
        assert_eq!(annotations.revision(), 2);

        assert!(annotations.undo());
        assert_eq!(origin(&annotations, 0), start);
        assert_eq!(annotations.items().len(), 1);
    }

    #[test]
    fn moves_without_distance_or_cancelled_change_nothing() {
        let mut annotations = Annotations::default();
        committed_text(&mut annotations, "Hi", (20.0, 50.0));
        let start = origin(&annotations, 0);

        annotations.begin_move(0);
        annotations.update_move((0.0, 0.0));
        assert!(!annotations.commit_move());

        annotations.begin_move(0);
        annotations.update_move((40.0, 40.0));
        annotations.cancel_move();
        assert_eq!(origin(&annotations, 0), start);
        assert_eq!(annotations.revision(), 1);

        // Shapes can't be moved.
        drag(&mut annotations, ShapeKind::Arrow, (0.0, 0.0), (20.0, 20.0));
        annotations.begin_move(1);
        assert!(!annotations.is_moving());
    }

    #[test]
    fn removing_texts_and_undoing_it() {
        let mut annotations = Annotations::default();
        committed_text(&mut annotations, "One", (20.0, 50.0));
        drag(&mut annotations, ShapeKind::Arrow, (0.0, 0.0), (20.0, 20.0));
        committed_text(&mut annotations, "Two", (20.0, 150.0));

        assert!(!annotations.remove_text(1), "not a text");
        assert!(annotations.remove_text(0));
        assert_eq!(annotations.items().len(), 2);

        annotations.begin_edit(1);
        annotations.insert("!");
        assert!(annotations.remove_edited_text());
        assert!(!annotations.is_editing_text());
        assert_eq!(annotations.items().len(), 1);
        assert!(matches!(annotations.items()[0], Annotation::Shape(_)));

        assert!(annotations.undo());
        assert!(matches!(&annotations.items()[1], Annotation::Text(t) if t.text == "Two"));
        assert!(annotations.undo());
        assert!(matches!(&annotations.items()[0], Annotation::Text(t) if t.text == "One"));
        assert_eq!(annotations.items().len(), 3);

        // A new text being typed is only dropped.
        annotations.begin_text(Color::Red, 20.0, (0.0, 0.0));
        annotations.insert("x");
        assert!(!annotations.remove_edited_text());
        assert_eq!(annotations.items().len(), 3);
    }

    #[test]
    fn undo_restores_an_edited_text() {
        let mut annotations = Annotations::default();
        committed_text(&mut annotations, "One", (20.0, 50.0));
        drag(&mut annotations, ShapeKind::Arrow, (0.0, 0.0), (20.0, 20.0));
        annotations.begin_edit(0);
        annotations.insert("!");
        assert!(annotations.commit_text());

        assert!(annotations.undo());
        assert_eq!(annotations.items().len(), 2, "the arrow stays");
        assert!(matches!(&annotations.items()[0], Annotation::Text(t) if t.text == "One"));
        assert!(annotations.can_undo());
    }
}
