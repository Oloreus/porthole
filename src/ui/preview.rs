use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::screenshot::Screenshot;
use crate::services::save::SaveOutcome;
use crate::services::{clipboard, save};
use crate::settings;
use crate::ui::zoom_view::ZoomView;

const MIN_WIDTH: i32 = 380;
const MIN_HEIGHT: i32 = 160;
/// Header- plus Aktionsleiste, für die Startgröße des Fensters.
const CHROME_HEIGHT: i32 = 96;

/// Öffnet ein eigenes Fenster für genau einen Screenshot. Das Fenster *ist*
/// der Zwischenspeicher: Schließen oder „Löschen" verwirft das Bild ohne
/// Rückfrage (es liegt nur im RAM; bereits Gespeichertes bleibt unberührt).
pub fn present(app: &impl IsA<gtk::Application>, screenshot: Screenshot) {
    let screenshot = Rc::new(screenshot);
    let (pixel_width, pixel_height) = (screenshot.texture.width(), screenshot.texture.height());

    // Startet eingepasst (kleine Bilder 1:1, nie hochskaliert); Zoom und Pan
    // betreffen nur die Anzeige, nie `screenshot`.
    let view = ZoomView::new(&screenshot.texture);
    view.add_css_class("porthole-preview");

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&view));

    let actions_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions_bar.add_css_class("toolbar");
    for (label, icon, action, tooltip) in [
        ("Kopieren", "edit-copy-symbolic", "preview.copy", "In die Zwischenablage kopieren (Strg+C)"),
        ("Speichern", "document-save-symbolic", "preview.save", "Als PNG speichern (Strg+S)"),
        ("Löschen", "user-trash-symbolic", "preview.delete", "Screenshot verwerfen (Entf)"),
    ] {
        let content = adw::ButtonContent::builder().label(label).icon_name(icon).build();
        let button = gtk::Button::builder()
            .child(&content)
            .action_name(action)
            .tooltip_text(tooltip)
            .build();
        actions_bar.append(&button);
    }
    let dimensions = gtk::Label::builder()
        .label(format!("{pixel_width} × {pixel_height}"))
        .hexpand(true)
        .halign(gtk::Align::End)
        .margin_end(6)
        .css_classes(["dim-label", "numeric"])
        .build();
    actions_bar.append(&dimensions);
    view.connect_zoom_changed(glib::clone!(
        #[weak]
        dimensions,
        move |zoom| {
            let percent = (zoom * 100.0).round();
            dimensions.set_label(&format!("{pixel_width} × {pixel_height}  ·  {percent} %"));
        }
    ));

    let layout = adw::ToolbarView::new();
    layout.add_top_bar(&adw::HeaderBar::new());
    layout.set_content(Some(&toasts));
    layout.add_bottom_bar(&actions_bar);

    let (width, height) = initial_size(pixel_width, pixel_height);
    let window = adw::Window::builder()
        .application(app)
        .title(screenshot.display_time())
        .content(&layout)
        .default_width(width)
        .default_height(height)
        .build();

    install_actions(&window, &toasts, &view, &screenshot);
    window.present();
}

fn install_actions(
    window: &adw::Window,
    toasts: &adw::ToastOverlay,
    view: &ZoomView,
    screenshot: &Rc<Screenshot>,
) {
    let copy = gio::ActionEntry::builder("copy")
        .activate(glib::clone!(
            #[weak]
            window,
            #[weak]
            toasts,
            #[strong]
            screenshot,
            move |_: &gio::SimpleActionGroup, _, _| {
                let display = WidgetExt::display(&window);
                let message = match clipboard::copy_image(&display, &screenshot) {
                    // The clipboard belongs to the display, not the window:
                    // the image stays available after closing.
                    Ok(()) if settings::get().close_preview_after_copy => {
                        window.destroy();
                        return;
                    }
                    Ok(()) => "In Zwischenablage kopiert".to_string(),
                    Err(err) => format!("Kopieren fehlgeschlagen: {err}"),
                };
                show_toast(&toasts, &message);
            }
        ))
        .build();

    let save = gio::ActionEntry::builder("save")
        .activate(glib::clone!(
            #[weak]
            window,
            #[weak]
            toasts,
            #[strong]
            screenshot,
            move |_: &gio::SimpleActionGroup, _, _| {
                glib::MainContext::default().spawn_local(glib::clone!(
                    #[weak]
                    window,
                    #[weak]
                    toasts,
                    #[strong]
                    screenshot,
                    async move {
                        match save::save_with_dialog(&window, &screenshot).await {
                            Ok(SaveOutcome::Saved(path)) => {
                                let name = path
                                    .file_name()
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                show_toast(&toasts, &format!("Gespeichert: {name}"));
                            }
                            Ok(SaveOutcome::Dismissed) => {}
                            // Preview bleibt offen – das Bild geht nicht verloren.
                            Err(err) => show_error(&window, &err),
                        }
                    }
                ));
            }
        ))
        .build();

    let delete = gio::ActionEntry::builder("delete")
        .activate(glib::clone!(
            #[weak]
            window,
            move |_: &gio::SimpleActionGroup, _, _| window.destroy()
        ))
        .build();

    let zoom_fit = gio::ActionEntry::builder("zoom-fit")
        .activate(glib::clone!(
            #[weak]
            view,
            move |_: &gio::SimpleActionGroup, _, _| view.zoom_fit()
        ))
        .build();

    let zoom_original = gio::ActionEntry::builder("zoom-original")
        .activate(glib::clone!(
            #[weak]
            view,
            move |_: &gio::SimpleActionGroup, _, _| view.zoom_original()
        ))
        .build();

    let group = gio::SimpleActionGroup::new();
    group.add_action_entries([copy, save, delete, zoom_fit, zoom_original]);
    window.insert_action_group("preview", Some(&group));

    let shortcuts = gtk::ShortcutController::new();
    for (trigger, action) in [
        ("<Control>c", "preview.copy"),
        ("<Control>s", "preview.save"),
        ("Delete", "preview.delete"),
        ("<Control>0|<Control>KP_0", "preview.zoom-fit"),
        ("<Control>1|<Control>KP_1", "preview.zoom-original"),
    ] {
        shortcuts.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string(trigger),
            Some(gtk::NamedAction::new(action)),
        ));
    }
    window.add_controller(shortcuts);
}

fn show_toast(toasts: &adw::ToastOverlay, message: &str) {
    toasts.add_toast(adw::Toast::builder().title(message).timeout(2).build());
}

fn show_error(window: &adw::Window, detail: &str) {
    let dialog = adw::AlertDialog::new(Some("Speichern fehlgeschlagen"), Some(detail));
    dialog.add_response("ok", "OK");
    dialog.present(Some(window));
}

/// Startgröße: Bild 1:1 (in logischen Pixeln) plus Leisten, begrenzt auf
/// 80 % des Monitors.
fn initial_size(pixel_width: i32, pixel_height: i32) -> (i32, i32) {
    let monitor = gdk::Display::default()
        .and_then(|display| display.monitors().item(0))
        .and_downcast::<gdk::Monitor>();
    let (scale, max_width, max_height) = match monitor {
        Some(monitor) => {
            let geometry = monitor.geometry();
            (
                monitor.scale(),
                (f64::from(geometry.width()) * 0.8) as i32,
                (f64::from(geometry.height()) * 0.8) as i32,
            )
        }
        None => (1.0, 1600, 900),
    };

    let width = (f64::from(pixel_width) / scale).ceil() as i32;
    let height = (f64::from(pixel_height) / scale).ceil() as i32 + CHROME_HEIGHT;
    (
        width.clamp(MIN_WIDTH, max_width.max(MIN_WIDTH)),
        height.clamp(MIN_HEIGHT, max_height.max(MIN_HEIGHT)),
    )
}
