//! Pure coordinate math, free of GTK – and therefore testable without hardware.
//!
//! Three coordinate spaces:
//! * **Stage**: Mutter's global space in which the monitors are arranged
//!   (logical pixels in the logical layout, physical ones in the physical layout).
//! * **Image**: pixels of the portal screenshot. It is *one* rendering of the
//!   whole stage, scaled uniformly by the largest monitor scale – the portal
//!   doesn't reveal the factor, it is derived from the sizes.
//! * **Window**: local coordinates of an overlay window covering exactly one
//!   monitor.
//!
//! Conversion uses ratios only, never reported scale factors – so it doesn't
//! matter what GDK reports under fractional scaling.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    /// Rectangle between any two corner points (drag in any direction).
    pub fn from_points(a: (f64, f64), b: (f64, f64)) -> Self {
        Self {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            width: (a.0 - b.0).abs(),
            height: (a.1 - b.1).abs(),
        }
    }

    pub fn clamp_to(self, bounds: Rect) -> Self {
        let left = self.x.clamp(bounds.x, bounds.x + bounds.width);
        let top = self.y.clamp(bounds.y, bounds.y + bounds.height);
        let right = (self.x + self.width).clamp(bounds.x, bounds.x + bounds.width);
        let bottom = (self.y + self.height).clamp(bounds.y, bounds.y + bounds.height);
        Self::new(left, top, right - left, bottom - top)
    }

    /// Overlap of two rectangles; `None` if it has no area (including when
    /// they merely touch along an edge).
    pub fn intersection(self, other: Rect) -> Option<Self> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        (right > left && bottom > top).then(|| Self::new(left, top, right - left, bottom - top))
    }

    /// Whether the point lies in the rectangle, edges included.
    pub fn contains(self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }

    pub fn union(self, other: Rect) -> Self {
        let left = self.x.min(other.x);
        let top = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Self::new(left, top, right - left, bottom - top)
    }
}

impl PixelRect {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }
}

#[derive(Debug, PartialEq)]
pub enum LayoutError {
    NoMonitors,
    /// Image and monitor layout don't match (e.g. the layout changed between
    /// capture and query). Better to abort than to crop wrongly.
    Mismatch { scale_x: f64, scale_y: f64 },
}

/// Image region per monitor, in the order of `monitors` (stage rectangles).
pub fn monitor_regions(
    monitors: &[Rect],
    image_size: (i32, i32),
) -> Result<Vec<PixelRect>, LayoutError> {
    let stage = monitors
        .iter()
        .copied()
        .reduce(Rect::union)
        .ok_or(LayoutError::NoMonitors)?;

    let scale_x = f64::from(image_size.0) / stage.width;
    let scale_y = f64::from(image_size.1) / stage.height;
    // Tolerance: one pixel of rounding across the whole image edge.
    let tolerance = 1.0 / stage.width.min(stage.height);
    if (scale_x - scale_y).abs() > tolerance.max(1e-3) {
        return Err(LayoutError::Mismatch { scale_x, scale_y });
    }

    let image = Rect::new(0.0, 0.0, f64::from(image_size.0), f64::from(image_size.1));
    Ok(monitors
        .iter()
        .map(|monitor| {
            let region = Rect::new(
                (monitor.x - stage.x) * scale_x,
                (monitor.y - stage.y) * scale_y,
                monitor.width * scale_x,
                monitor.height * scale_y,
            );
            round_nearest(region.clamp_to(image))
        })
        .collect())
}

/// Selection in window coordinates → image pixels within the monitor region.
///
/// Rounds exactly once, outward (origin down, end up): every pixel that is
/// even partially covered belongs to the selection.
pub fn selection_to_pixels(
    selection: Rect,
    window_size: (f64, f64),
    region: PixelRect,
) -> PixelRect {
    if window_size.0 <= 0.0 || window_size.1 <= 0.0 {
        return PixelRect::new(region.x, region.y, 0, 0);
    }
    let scale_x = f64::from(region.width) / window_size.0;
    let scale_y = f64::from(region.height) / window_size.1;
    let selection = selection.clamp_to(Rect::new(0.0, 0.0, window_size.0, window_size.1));

    // Epsilon absorbs floating-point noise (e.g. 100.00000001 → 101 pixels).
    const EPSILON: f64 = 1e-6;
    let left = (selection.x * scale_x + EPSILON).floor() as i32;
    let top = (selection.y * scale_y + EPSILON).floor() as i32;
    let right = ((selection.x + selection.width) * scale_x - EPSILON).ceil() as i32;
    let bottom = ((selection.y + selection.height) * scale_y - EPSILON).ceil() as i32;

    let left = left.clamp(0, region.width);
    let top = top.clamp(0, region.height);
    let right = right.clamp(left, region.width);
    let bottom = bottom.clamp(top, region.height);
    PixelRect::new(region.x + left, region.y + top, right - left, bottom - top)
}

fn round_nearest(rect: Rect) -> PixelRect {
    let left = rect.x.round() as i32;
    let top = rect.y.round() as i32;
    let right = (rect.x + rect.width).round() as i32;
    let bottom = (rect.y + rect.height).round() as i32;
    PixelRect::new(left, top, right - left, bottom - top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_in_any_direction_normalizes() {
        let expected = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(Rect::from_points((10.0, 20.0), (40.0, 60.0)), expected);
        assert_eq!(Rect::from_points((40.0, 60.0), (10.0, 20.0)), expected);
        assert_eq!(Rect::from_points((40.0, 20.0), (10.0, 60.0)), expected);
    }

    #[test]
    fn drag_beyond_window_is_clamped() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
        let rect = Rect::from_points((50.0, 50.0), (-30.0, 180.0)).clamp_to(bounds);
        assert_eq!(rect, Rect::new(0.0, 50.0, 50.0, 50.0));
    }

    #[test]
    fn intersection_ignores_touching_edges() {
        let left = Rect::new(0.0, 0.0, 100.0, 100.0);
        let right = Rect::new(100.0, 0.0, 100.0, 100.0);
        assert_eq!(left.intersection(right), None);
        assert_eq!(
            Rect::new(50.0, 10.0, 100.0, 20.0).intersection(right),
            Some(Rect::new(100.0, 10.0, 50.0, 20.0))
        );
    }

    #[test]
    fn contains_includes_edges() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(rect.contains((100.0, 100.0)));
        assert!(!rect.contains((100.5, 50.0)));
    }

    #[test]
    fn single_monitor_covers_whole_image() {
        let regions = monitor_regions(&[Rect::new(0.0, 0.0, 2560.0, 1440.0)], (2560, 1440));
        assert_eq!(regions, Ok(vec![PixelRect::new(0, 0, 2560, 1440)]));
    }

    #[test]
    fn single_monitor_at_200_percent() {
        // Logical layout: stage 1280x720, image in physical pixels.
        let regions = monitor_regions(&[Rect::new(0.0, 0.0, 1280.0, 720.0)], (2560, 1440));
        assert_eq!(regions, Ok(vec![PixelRect::new(0, 0, 2560, 1440)]));
    }

    #[test]
    fn mixed_scales_logical_layout() {
        // 2560x1440 @100 % on the left, 3840x2160 @150 % on the right (logical 2560x1440).
        // Stage 5120x1440, portal renders at max scale 1.5 → 7680x2160.
        let monitors = [
            Rect::new(0.0, 0.0, 2560.0, 1440.0),
            Rect::new(2560.0, 0.0, 2560.0, 1440.0),
        ];
        let regions = monitor_regions(&monitors, (7680, 2160)).unwrap();
        assert_eq!(regions[0], PixelRect::new(0, 0, 3840, 2160));
        assert_eq!(regions[1], PixelRect::new(3840, 0, 3840, 2160));
    }

    #[test]
    fn mixed_resolutions_physical_layout() {
        // Physical layout: stage in physical pixels, factor 1.
        let monitors = [
            Rect::new(0.0, 0.0, 2560.0, 1440.0),
            Rect::new(2560.0, 0.0, 3840.0, 2160.0),
        ];
        let regions = monitor_regions(&monitors, (6400, 2160)).unwrap();
        assert_eq!(regions[0], PixelRect::new(0, 0, 2560, 1440));
        assert_eq!(regions[1], PixelRect::new(2560, 0, 3840, 2160));
    }

    #[test]
    fn offset_arrangement_with_negative_origin() {
        // Second monitor to the upper left: stage doesn't start at (0,0).
        let monitors = [
            Rect::new(0.0, 0.0, 1920.0, 1080.0),
            Rect::new(-1920.0, -200.0, 1920.0, 1080.0),
        ];
        let regions = monitor_regions(&monitors, (3840, 1280)).unwrap();
        assert_eq!(regions[0], PixelRect::new(1920, 200, 1920, 1080));
        assert_eq!(regions[1], PixelRect::new(0, 0, 1920, 1080));
    }

    #[test]
    fn layout_mismatch_is_an_error() {
        let result = monitor_regions(&[Rect::new(0.0, 0.0, 2560.0, 1440.0)], (2560, 1600));
        assert!(matches!(result, Err(LayoutError::Mismatch { .. })));
        assert_eq!(monitor_regions(&[], (100, 100)), Err(LayoutError::NoMonitors));
    }

    #[test]
    fn selection_at_100_percent_is_exact() {
        let region = PixelRect::new(0, 0, 2560, 1440);
        let pixels = selection_to_pixels(
            Rect::new(100.0, 200.0, 525.0, 493.0),
            (2560.0, 1440.0),
            region,
        );
        assert_eq!(pixels, PixelRect::new(100, 200, 525, 493));
    }

    #[test]
    fn selection_at_200_percent_doubles() {
        let region = PixelRect::new(0, 0, 2560, 1440);
        let pixels =
            selection_to_pixels(Rect::new(10.0, 20.0, 100.0, 50.0), (1280.0, 720.0), region);
        assert_eq!(pixels, PixelRect::new(20, 40, 200, 100));
    }

    #[test]
    fn fractional_selection_rounds_outward() {
        // 150 %: window 1707x960 logical on 2560x1440 pixels.
        let region = PixelRect::new(0, 0, 2560, 1440);
        let pixels = selection_to_pixels(
            Rect::new(10.3, 10.3, 100.2, 100.2),
            (2560.0 / 1.5, 1440.0 / 1.5),
            region,
        );
        assert_eq!(pixels, PixelRect::new(15, 15, 151, 151));
    }

    #[test]
    fn selection_is_offset_into_second_monitor_region() {
        let region = PixelRect::new(3840, 0, 3840, 2160);
        let pixels =
            selection_to_pixels(Rect::new(0.0, 0.0, 100.0, 100.0), (2560.0, 1440.0), region);
        assert_eq!(pixels, PixelRect::new(3840, 0, 150, 150));
    }

    #[test]
    fn selection_never_leaves_region() {
        let region = PixelRect::new(100, 100, 200, 200);
        let pixels =
            selection_to_pixels(Rect::new(-50.0, 150.0, 500.0, 500.0), (200.0, 200.0), region);
        assert_eq!(pixels, PixelRect::new(100, 250, 200, 50));
        assert!(selection_to_pixels(Rect::new(5.0, 5.0, 0.0, 0.0), (200.0, 200.0), region).is_empty());
    }
}
