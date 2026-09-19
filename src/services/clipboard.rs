use gtk::gdk;
use gtk::prelude::*;

use crate::screenshot::Screenshot;

/// Legt den Screenshot als *Bild* in die Zwischenablage.
///
/// * `image/png` kommt aus den fertig kodierten Bytes – genau das, was
///   Chromium/Electron (Slack, Browser) verlangen, ohne Wartezeit beim Einfügen.
/// * Der Texture-Provider ergänzt weitere Bildformate, die GDK erst bei Bedarf
///   kodiert.
/// * Bewusst kein Text/Dateipfad: Mutters Clipboard-Manager würde sonst beim
///   Beenden der App den Text statt des Bildes aufbewahren.
///
/// Mutter nimmt die Auswahl nur vom Client mit Tastaturfokus an; der Aufruf
/// muss daher aus einer Nutzeraktion im fokussierten Fenster stammen. Der
/// Provider hält eigene Referenzen – das Preview darf danach geschlossen werden.
pub fn copy_image(display: &gdk::Display, screenshot: &Screenshot) -> Result<(), String> {
    let png = gdk::ContentProvider::for_bytes("image/png", &screenshot.png);
    let other_formats = gdk::ContentProvider::for_value(&screenshot.texture.to_value());
    let provider = gdk::ContentProvider::new_union(&[png, other_formats]);

    display
        .clipboard()
        .set_content(Some(&provider))
        .map_err(|err| err.to_string())
}
