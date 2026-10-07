use gtk::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk};

use crate::annotations::{self, Shape};
use crate::geometry::PixelRect;

const FORMAT: gdk::MemoryFormat = gdk::MemoryFormat::R8g8b8a8;
const BYTES_PER_PIXEL: usize = 4;

/// Ein aufgenommener Ausschnitt. Lebt ausschließlich im RAM – „Löschen" heißt,
/// diesen Wert fallen zu lassen; es gibt keine temporären Dateien.
pub struct Screenshot {
    pub texture: gdk::Texture,
    /// Einmalig kodiert: dieselben Bytes gehen in die Zwischenablage und in
    /// die gespeicherte Datei.
    pub png: glib::Bytes,
    pub taken_at: glib::DateTime,
}

impl Screenshot {
    pub async fn from_frame(frame: &gdk::Texture, rect: PixelRect) -> Option<Self> {
        let texture = crop(frame, rect)?;
        let taken_at = glib::DateTime::now_local().ok()?;

        let encoder_input = texture.clone();
        let png = gio::spawn_blocking(move || encoder_input.save_to_png_bytes())
            .await
            .ok()?;

        Some(Self { texture, png, taken_at })
    }

    /// The same screenshot with `shapes` burned into the pixels, at full
    /// resolution. Synchronous on purpose: copying must happen within the
    /// user action, see `services::clipboard`.
    pub fn with_annotations(&self, shapes: &[Shape]) -> Option<Self> {
        annotations::register_node_types();
        // Software rendering: needs no window or GPU, and its result is
        // already in memory for the PNG encoder.
        let renderer = gsk::CairoRenderer::new();
        renderer.realize(None).ok()?;

        let (width, height) = (self.texture.width(), self.texture.height());
        let bounds = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
        let mut nodes = vec![gsk::TextureNode::new(&self.texture, &bounds).upcast()];
        nodes.extend(annotations::render_nodes(shapes));
        let root = gsk::ContainerNode::new(&nodes);

        let texture = renderer.render_texture(&root, Some(&bounds));
        renderer.unrealize();

        let png = texture.save_to_png_bytes();
        Some(Self { texture, png, taken_at: self.taken_at.clone() })
    }

    /// z. B. `Screenshot_2026-09-19_09-28-00.png`
    pub fn suggested_filename(&self) -> String {
        let stamp = self
            .taken_at
            .format("%Y-%m-%d_%H-%M-%S")
            .map(|stamp| stamp.to_string())
            .unwrap_or_else(|_| "unbenannt".into());
        format!("Screenshot_{stamp}.png")
    }

    pub fn display_time(&self) -> String {
        self.taken_at
            .format("%d.%m.%Y %H:%M:%S")
            .map(|stamp| stamp.to_string())
            .unwrap_or_default()
    }
}

/// Kopiert den Ausschnitt in einen eigenen Puffer, damit ein kleines Preview
/// nicht den kompletten Desktop-Frame (viele MB) am Leben hält.
fn crop(frame: &gdk::Texture, rect: PixelRect) -> Option<gdk::Texture> {
    let (pixels, stride) = download(frame);
    let bounds_ok = rect.x >= 0
        && rect.y >= 0
        && !rect.is_empty()
        && rect.x + rect.width <= frame.width()
        && rect.y + rect.height <= frame.height();
    if !bounds_ok {
        return None;
    }

    let (x, y) = (rect.x as usize, rect.y as usize);
    let (width, height) = (rect.width as usize, rect.height as usize);
    let row_len = width * BYTES_PER_PIXEL;

    let mut cropped = Vec::with_capacity(row_len * height);
    for row in y..y + height {
        let start = row * stride + x * BYTES_PER_PIXEL;
        cropped.extend_from_slice(&pixels[start..start + row_len]);
    }

    let texture = gdk::MemoryTexture::new(
        rect.width,
        rect.height,
        FORMAT,
        &glib::Bytes::from_owned(cropped),
        row_len,
    );
    Some(texture.upcast())
}

/// Teilbild ohne Kopie (teilt sich den Puffer mit dem Frame) – für die
/// kurzlebige Anzeige je Monitor im Auswahl-Overlay.
pub fn view(frame: &gdk::Texture, rect: PixelRect) -> gdk::Texture {
    if rect == PixelRect::new(0, 0, frame.width(), frame.height()) {
        return frame.clone();
    }
    let (pixels, stride) = download(frame);
    let start = rect.y as usize * stride + rect.x as usize * BYTES_PER_PIXEL;
    let end = start
        + (rect.height as usize - 1) * stride
        + rect.width as usize * BYTES_PER_PIXEL;
    let window = glib::Bytes::from_bytes(&pixels, start..end);
    gdk::MemoryTexture::new(rect.width, rect.height, FORMAT, &window, stride).upcast()
}

fn download(texture: &gdk::Texture) -> (glib::Bytes, usize) {
    let mut downloader = gdk::TextureDownloader::new(texture);
    downloader.set_format(FORMAT);
    downloader.download_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4x3-Testbild: Pixel (x, y) hat die Farbe (x, y, 0, 255).
    fn test_frame() -> gdk::Texture {
        let mut pixels = Vec::new();
        for y in 0..3u8 {
            for x in 0..4u8 {
                pixels.extend_from_slice(&[x, y, 0, 255]);
            }
        }
        gdk::MemoryTexture::new(4, 3, FORMAT, &glib::Bytes::from_owned(pixels), 16).upcast()
    }

    fn pixels_of(texture: &gdk::Texture) -> Vec<(u8, u8)> {
        let (bytes, stride) = download(texture);
        let mut result = Vec::new();
        for row in 0..texture.height() as usize {
            for column in 0..texture.width() as usize {
                let offset = row * stride + column * BYTES_PER_PIXEL;
                result.push((bytes[offset], bytes[offset + 1]));
            }
        }
        result
    }

    #[test]
    fn crop_copies_exactly_the_selected_pixels() {
        let cropped = crop(&test_frame(), PixelRect::new(1, 1, 2, 2)).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (2, 2));
        assert_eq!(pixels_of(&cropped), vec![(1, 1), (2, 1), (1, 2), (2, 2)]);
    }

    #[test]
    fn crop_rejects_out_of_bounds_and_empty() {
        assert!(crop(&test_frame(), PixelRect::new(3, 0, 2, 1)).is_none());
        assert!(crop(&test_frame(), PixelRect::new(-1, 0, 2, 1)).is_none());
        assert!(crop(&test_frame(), PixelRect::new(0, 0, 0, 1)).is_none());
    }

    #[test]
    fn view_shares_buffer_but_shows_the_right_region() {
        let frame = test_frame();
        let view = view(&frame, PixelRect::new(2, 1, 2, 2));
        assert_eq!(pixels_of(&view), vec![(2, 1), (3, 1), (2, 2), (3, 2)]);
        // Bis in die letzte Ecke (Puffer-Ende) darf nichts überlaufen.
        let full = super::view(&frame, PixelRect::new(0, 0, 4, 3));
        assert_eq!(pixels_of(&full).len(), 12);
    }

    #[test]
    fn annotations_are_burned_in_at_full_size() {
        use crate::annotations::{Color, ShapeKind};

        let pixels = vec![0u8; 40 * 30 * BYTES_PER_PIXEL];
        let texture: gdk::Texture =
            gdk::MemoryTexture::new(40, 30, FORMAT, &glib::Bytes::from_owned(pixels), 40 * 4).upcast();
        let screenshot = Screenshot {
            png: texture.save_to_png_bytes(),
            texture,
            taken_at: glib::DateTime::now_local().unwrap(),
        };
        let shape = Shape {
            kind: ShapeKind::Rectangle,
            from: (10.0, 10.0),
            to: (30.0, 20.0),
            color: Color::Green,
            width: 2.0,
        };

        let annotated = screenshot.with_annotations(&[shape]).unwrap();
        assert_eq!((annotated.texture.width(), annotated.texture.height()), (40, 30));
        let (bytes, stride) = download(&annotated.texture);
        let at = |x: usize, y: usize| {
            let offset = y * stride + x * BYTES_PER_PIXEL;
            (bytes[offset], bytes[offset + 1], bytes[offset + 2])
        };
        // On the left edge of the rectangle: green (#2ec27e).
        assert_eq!(at(10, 15), (0x2e, 0xc2, 0x7e));
        // Inside and outside: untouched.
        assert_eq!(at(20, 15), (0, 0, 0));
        assert_eq!(at(2, 2), (0, 0, 0));
        let decoded = gdk::Texture::from_bytes(&annotated.png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (40, 30));
    }

    #[test]
    fn png_roundtrip_keeps_dimensions() {
        let cropped = crop(&test_frame(), PixelRect::new(0, 0, 3, 2)).unwrap();
        let png = cropped.save_to_png_bytes();
        assert_eq!(&png[1..4], b"PNG");
        let decoded = gdk::Texture::from_bytes(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (3, 2));
    }
}
