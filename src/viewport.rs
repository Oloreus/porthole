//! Zoom- und Pan-Zustand der Vorschau, frei von GTK – dadurch ohne Display
//! testbar.
//!
//! Alle Längen sind Widget-Einheiten (logische Pixel). `image` ist die
//! Bildgröße bei 100 %, also Bildpixel geteilt durch den Surface-Scale:
//! 100 % heißt ein Bildpixel auf einem Bildschirmpixel.
//!
//! Invariante: Nach jeder Änderung liegt `pan` in seinen Grenzen. Ist das Bild
//! auf einer Achse kleiner als der Viewport, wird es dort zentriert; sonst
//! schließt es mit den Viewport-Kanten ab – leerer Rand ist nie sichtbar.

use crate::geometry::Rect;

pub const MAX_ZOOM: f64 = 5.0;
/// So weit lässt sich von Hand herauszoomen – auch unter das Einpassen.
pub const MIN_MANUAL_ZOOM: f64 = 0.1;
/// Untergrenze auch für das Einpassen: der Zoom wird nie 0 oder negativ.
pub const MIN_ZOOM: f64 = 0.01;
/// Faktor je Mausrad-Raste.
pub const ZOOM_STEP: f64 = 1.1;

/// Überstand, ab dem eine Achse als verschiebbar gilt (fängt
/// Fließkomma-Rauschen beim Einpassen ab).
const SLACK: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZoomMode {
    /// Ganzes Bild sichtbar, nie hochskaliert; folgt der Fenstergröße.
    Fit,
    /// Vom Nutzer gewählter Faktor; bleibt beim Ändern der Fenstergröße stehen.
    Manual(f64),
}

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    image: (f64, f64),
    size: (f64, f64),
    mode: ZoomMode,
    /// Linke obere Bildecke in Viewport-Koordinaten.
    pan: (f64, f64),
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            image: (0.0, 0.0),
            size: (0.0, 0.0),
            mode: ZoomMode::Fit,
            pan: (0.0, 0.0),
        }
    }
}

impl Viewport {
    pub fn mode(&self) -> ZoomMode {
        self.mode
    }

    pub fn zoom(&self) -> f64 {
        match self.mode {
            ZoomMode::Fit => self.fit_zoom(),
            ZoomMode::Manual(zoom) => zoom,
        }
    }

    /// Bewusst nicht gespeichert: aus Bild und Viewport berechnet, kann der
    /// Wert nie veralten.
    pub fn fit_zoom(&self) -> f64 {
        let (image_width, image_height) = self.image;
        let (width, height) = self.size;
        if image_width <= 0.0 || image_height <= 0.0 || width <= 0.0 || height <= 0.0 {
            return 1.0;
        }
        (width / image_width)
            .min(height / image_height)
            .clamp(MIN_ZOOM, 1.0)
    }

    pub fn pan(&self) -> (f64, f64) {
        self.pan
    }

    /// Wo das Bild im Viewport gezeichnet wird.
    pub fn image_rect(&self) -> Rect {
        let zoom = self.zoom();
        Rect::new(self.pan.0, self.pan.1, self.image.0 * zoom, self.image.1 * zoom)
    }

    pub fn is_pannable(&self) -> bool {
        let rect = self.image_rect();
        rect.width > self.size.0 + SLACK || rect.height > self.size.1 + SLACK
    }

    pub fn set_image(&mut self, width: f64, height: f64) {
        self.image = (width, height);
        self.clamp();
    }

    /// Im Fit-Modus wird neu eingepasst. Nach manuellem Zoom bleibt der Faktor
    /// stehen und der Bildpunkt in der Viewport-Mitte an seinem Platz.
    pub fn set_size(&mut self, width: f64, height: f64) {
        let (old_width, old_height) = self.size;
        self.size = (width, height);
        if let ZoomMode::Manual(_) = self.mode {
            self.pan.0 += (width - old_width) / 2.0;
            self.pan.1 += (height - old_height) / 2.0;
        }
        self.clamp();
    }

    pub fn set_fit(&mut self) {
        self.mode = ZoomMode::Fit;
        self.clamp();
    }

    /// Ein Zoomschritt um `factor`; der Bildpunkt unter `anchor` bleibt stehen.
    ///
    /// Herauszoomen endet bei [`MIN_MANUAL_ZOOM`], Hineinzoomen bei
    /// [`MAX_ZOOM`]. Wird 100 % oder das Einpassen überquert, rastet der
    /// Schritt dort ein – so sind die pixelgenaue Ansicht und der Fit-Modus
    /// mit dem Mausrad erreichbar.
    pub fn zoom_at(&mut self, anchor: (f64, f64), factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let old = self.zoom();
        let fit = self.fit_zoom();
        // Riesige Bilder sind schon eingepasst kleiner als das Minimum; ein
        // Schritt darf nie in die Gegenrichtung zwingen.
        let min = MIN_MANUAL_ZOOM.min(fit).min(old);
        let zoom = (old * factor).clamp(min, MAX_ZOOM.max(old));

        // Bei mehreren überquerten Rastpunkten gilt der erste auf dem Weg.
        let crossed = |stop: &f64| (old < *stop && zoom > *stop) || (old > *stop && zoom < *stop);
        let zoom = [1.0, fit]
            .into_iter()
            .filter(crossed)
            .min_by(|a, b| (a - old).abs().total_cmp(&(b - old).abs()))
            .unwrap_or(zoom);
        self.set_zoom_at(anchor, zoom);
    }

    /// Setzt den Zoom absolut, verankert am Punkt `anchor` des Viewports.
    pub fn set_zoom_at(&mut self, anchor: (f64, f64), zoom: f64) {
        if !zoom.is_finite() {
            return;
        }
        let old = self.zoom();
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);

        // Der Bildpunkt unter dem Anker, p = (anchor − pan) / old, soll nach
        // dem Zoom wieder unter dem Anker liegen: pan' = anchor − p · zoom.
        let ratio = zoom / old;
        self.pan.0 = anchor.0 - (anchor.0 - self.pan.0) * ratio;
        self.pan.1 = anchor.1 - (anchor.1 - self.pan.1) * ratio;

        // Beim Einpassen angekommen: wieder der Fenstergröße folgen.
        let fit = self.fit_zoom();
        self.mode = if (zoom - fit).abs() <= fit * 1e-6 {
            ZoomMode::Fit
        } else {
            ZoomMode::Manual(zoom)
        };
        self.clamp();
    }

    pub fn pan_to(&mut self, x: f64, y: f64) {
        self.pan = (x, y);
        self.clamp();
    }

    pub fn pan_by(&mut self, dx: f64, dy: f64) {
        self.pan_to(self.pan.0 + dx, self.pan.1 + dy);
    }

    fn clamp(&mut self) {
        let zoom = self.zoom();
        self.pan = (
            clamp_axis(self.pan.0, self.image.0 * zoom, self.size.0),
            clamp_axis(self.pan.1, self.image.1 * zoom, self.size.1),
        );
    }
}

fn clamp_axis(pan: f64, rendered: f64, viewport: f64) -> f64 {
    if rendered <= viewport + SLACK {
        (viewport - rendered) / 2.0
    } else {
        pan.clamp(viewport - rendered, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f64 = 1e-9;

    fn viewport(image: (f64, f64), size: (f64, f64)) -> Viewport {
        let mut viewport = Viewport::default();
        viewport.set_image(image.0, image.1);
        viewport.set_size(size.0, size.1);
        viewport
    }

    /// Beispiel aus der Praxis: 2494 × 1285 im Fenster mit ca. 1600 × 900.
    fn large() -> Viewport {
        viewport((2494.0, 1285.0), (1600.0, 900.0))
    }

    fn center(viewport: &Viewport) -> (f64, f64) {
        (viewport.size.0 / 2.0, viewport.size.1 / 2.0)
    }

    fn image_point_at(viewport: &Viewport, point: (f64, f64)) -> (f64, f64) {
        let rect = viewport.image_rect();
        let zoom = viewport.zoom();
        ((point.0 - rect.x) / zoom, (point.1 - rect.y) / zoom)
    }

    fn assert_within_bounds(viewport: &Viewport) {
        let rect = viewport.image_rect();
        for (pan, rendered, size) in [
            (rect.x, rect.width, viewport.size.0),
            (rect.y, rect.height, viewport.size.1),
        ] {
            if rendered <= size + SLACK {
                assert!((pan - (size - rendered) / 2.0).abs() < EPSILON, "nicht zentriert");
            } else {
                assert!(pan <= 0.0 && pan + rendered >= size, "leerer Rand sichtbar");
            }
        }
    }

    #[test]
    fn fit_scales_large_image_down_and_centers_it() {
        let viewport = large();
        assert_eq!(viewport.mode(), ZoomMode::Fit);
        assert!((viewport.zoom() - 1600.0 / 2494.0).abs() < EPSILON);
        let rect = viewport.image_rect();
        assert!((rect.width - 1600.0).abs() < 1e-6);
        assert!((rect.y - (900.0 - rect.height) / 2.0).abs() < EPSILON);
        assert!(!viewport.is_pannable());
    }

    #[test]
    fn fit_never_scales_up() {
        let viewport = viewport((200.0, 100.0), (1600.0, 900.0));
        assert_eq!(viewport.zoom(), 1.0);
        assert_eq!(viewport.image_rect(), Rect::new(700.0, 400.0, 200.0, 100.0));
    }

    #[test]
    fn zoom_stays_positive_and_bounded() {
        let mut unallocated = viewport((2494.0, 1285.0), (0.0, 0.0));
        assert_eq!(unallocated.zoom(), 1.0);
        unallocated.zoom_at((0.0, 0.0), 0.0);
        unallocated.zoom_at((0.0, 0.0), -3.0);
        unallocated.zoom_at((0.0, 0.0), f64::NAN);
        assert_eq!(unallocated.zoom(), 1.0);

        let mut tiny_window = viewport((100_000.0, 100_000.0), (1.0, 1.0));
        assert_eq!(tiny_window.zoom(), MIN_ZOOM);
        for _ in 0..200 {
            tiny_window.zoom_at((0.0, 0.0), 1.0 / ZOOM_STEP);
        }
        assert!(tiny_window.zoom() >= MIN_ZOOM);

        let mut viewport = large();
        for _ in 0..200 {
            viewport.zoom_at((800.0, 450.0), ZOOM_STEP);
        }
        assert_eq!(viewport.zoom(), MAX_ZOOM);
        assert_within_bounds(&viewport);
    }

    #[test]
    fn zoom_keeps_image_point_under_cursor() {
        let mut viewport = large();
        viewport.set_zoom_at(center(&viewport), 2.0);
        let cursor = (400.0, 300.0);
        let before = image_point_at(&viewport, cursor);
        viewport.zoom_at(cursor, ZOOM_STEP);
        viewport.zoom_at(cursor, ZOOM_STEP);
        let after = image_point_at(&viewport, cursor);
        assert!((viewport.zoom() - 2.0 * ZOOM_STEP * ZOOM_STEP).abs() < EPSILON);
        assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6);
    }

    #[test]
    fn first_zoom_from_fit_is_anchored_too() {
        let mut viewport = viewport((4000.0, 2000.0), (1000.0, 500.0));
        let cursor = (250.0, 125.0);
        let before = image_point_at(&viewport, cursor);
        viewport.zoom_at(cursor, ZOOM_STEP);
        assert!(matches!(viewport.mode(), ZoomMode::Manual(_)));
        let after = image_point_at(&viewport, cursor);
        assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6);
    }

    #[test]
    fn every_corner_is_reachable_but_not_beyond() {
        let mut viewport = large();
        viewport.set_zoom_at(center(&viewport), 3.0);
        assert!(viewport.is_pannable());
        let (width, height) = (2494.0 * 3.0, 1285.0 * 3.0);

        viewport.pan_to(10_000.0, 10_000.0);
        assert_eq!(viewport.pan(), (0.0, 0.0));
        viewport.pan_to(-100_000.0, -100_000.0);
        assert_eq!(viewport.pan(), (1600.0 - width, 900.0 - height));
        viewport.pan_by(-500.0, 250.0);
        assert_eq!(viewport.pan(), (1600.0 - width, 900.0 - height + 250.0));
        assert_within_bounds(&viewport);
    }

    #[test]
    fn zooming_out_after_panning_to_the_corner_stays_in_bounds() {
        let mut viewport = large();
        viewport.set_zoom_at(center(&viewport), 4.0);
        viewport.pan_to(-100_000.0, -100_000.0);

        viewport.set_zoom_at((0.0, 0.0), 1.0);
        assert_within_bounds(&viewport);

        viewport.pan_to(-100_000.0, -100_000.0);
        for _ in 0..100 {
            viewport.zoom_at((0.0, 0.0), 1.0 / ZOOM_STEP);
            assert_within_bounds(&viewport);
        }
    }

    #[test]
    fn smaller_axis_is_centered_while_the_other_pans() {
        // Breiter, aber niedriger als der Viewport.
        let mut viewport = viewport((3000.0, 300.0), (1000.0, 800.0));
        viewport.set_zoom_at(center(&viewport), 1.0);
        assert!(viewport.is_pannable());
        viewport.pan_by(-400.0, -400.0);
        assert_eq!(viewport.pan(), (-1400.0, 250.0));
        viewport.pan_by(0.0, 10_000.0);
        assert_eq!(viewport.pan().1, 250.0);
    }

    #[test]
    fn resize_refits_in_fit_mode() {
        let mut viewport = large();
        viewport.set_size(800.0, 900.0);
        assert_eq!(viewport.mode(), ZoomMode::Fit);
        assert!((viewport.zoom() - 800.0 / 2494.0).abs() < EPSILON);
        assert_within_bounds(&viewport);
    }

    #[test]
    fn resize_keeps_manual_zoom_and_clamps_pan() {
        let mut viewport = large();
        viewport.set_zoom_at(center(&viewport), 2.0);
        viewport.pan_to(-100_000.0, -100_000.0);
        let centered_before = image_point_at(&viewport, center(&viewport));

        viewport.set_size(1400.0, 800.0);
        assert_eq!(viewport.mode(), ZoomMode::Manual(2.0));
        let centered_after = image_point_at(&viewport, center(&viewport));
        assert!((centered_before.0 - centered_after.0).abs() < 1e-6);

        // Größer als das gezoomte Bild: Zoom bleibt, Bild wird zentriert.
        viewport.set_size(6000.0, 3000.0);
        assert_eq!(viewport.mode(), ZoomMode::Manual(2.0));
        assert!(!viewport.is_pannable());
        assert_within_bounds(&viewport);
    }

    #[test]
    fn zooming_out_returns_to_fit() {
        let mut viewport = large();
        let fit = viewport.zoom();
        viewport.zoom_at((100.0, 100.0), ZOOM_STEP);
        viewport.zoom_at((100.0, 100.0), ZOOM_STEP);
        assert!(matches!(viewport.mode(), ZoomMode::Manual(_)));
        viewport.zoom_at((100.0, 100.0), 1.0 / ZOOM_STEP);
        viewport.zoom_at((100.0, 100.0), 1.0 / ZOOM_STEP);
        assert_eq!(viewport.mode(), ZoomMode::Fit);
        assert_eq!(viewport.zoom(), fit);
    }

    #[test]
    fn zooming_out_continues_below_fit_down_to_the_minimum() {
        // Fenster in Bildgröße: eingepasst ist bereits 100 %.
        let mut viewport = viewport((800.0, 600.0), (800.0, 600.0));
        assert_eq!(viewport.zoom(), 1.0);
        viewport.zoom_at((100.0, 100.0), 1.0 / ZOOM_STEP);
        assert!(matches!(viewport.mode(), ZoomMode::Manual(zoom) if zoom < 1.0));
        assert_within_bounds(&viewport);

        for _ in 0..100 {
            viewport.zoom_at((100.0, 100.0), 1.0 / ZOOM_STEP);
        }
        assert_eq!(viewport.zoom(), MIN_MANUAL_ZOOM);
        assert!(!viewport.is_pannable());
        assert_within_bounds(&viewport);
    }

    #[test]
    fn crossing_fit_snaps_back_into_fit_mode() {
        let mut viewport = large();
        let fit = viewport.zoom();
        viewport.zoom_at(center(&viewport), 1.0 / ZOOM_STEP);
        viewport.zoom_at(center(&viewport), 1.0 / ZOOM_STEP);
        assert!(viewport.zoom() < fit);
        // Ein großer Schritt überquert Einpassen und 100 %: das Nähere gilt.
        viewport.zoom_at(center(&viewport), 3.0);
        assert_eq!(viewport.mode(), ZoomMode::Fit);
        viewport.zoom_at(center(&viewport), 3.0);
        assert_eq!(viewport.zoom(), 1.0);
    }

    #[test]
    fn zoom_below_fit_does_not_jump_when_zooming_in() {
        let mut viewport = large();
        viewport.set_zoom_at(center(&viewport), 0.8);
        viewport.set_size(4000.0, 2000.0);
        assert_eq!(viewport.fit_zoom(), 1.0);
        assert_eq!(viewport.mode(), ZoomMode::Manual(0.8));

        viewport.zoom_at(center(&viewport), ZOOM_STEP);
        assert!((viewport.zoom() - 0.88).abs() < EPSILON);
    }

    #[test]
    fn huge_image_never_zooms_in_on_a_zoom_out_step() {
        let mut viewport = viewport((100_000.0, 50_000.0), (1000.0, 500.0));
        let fit = viewport.zoom();
        assert!(fit < MIN_MANUAL_ZOOM);
        viewport.zoom_at((0.0, 0.0), 1.0 / ZOOM_STEP);
        assert_eq!(viewport.zoom(), fit);
    }

    #[test]
    fn crossing_100_percent_snaps() {
        let mut viewport = large();
        viewport.set_zoom_at(center(&viewport), 0.95);
        viewport.zoom_at(center(&viewport), ZOOM_STEP);
        assert_eq!(viewport.zoom(), 1.0);
        viewport.zoom_at(center(&viewport), ZOOM_STEP);
        viewport.zoom_at(center(&viewport), 1.0 / (ZOOM_STEP * ZOOM_STEP * ZOOM_STEP));
        assert_eq!(viewport.zoom(), 1.0);
    }
}
