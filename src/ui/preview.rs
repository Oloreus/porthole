use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::annotations::{Color, ShapeKind, Tool};
use crate::i18n::tr;
use crate::screenshot::Screenshot;
use crate::services::save::SaveOutcome;
use crate::services::{clipboard, save};
use crate::ui::tool_icon::ToolIcon;
use crate::ui::zoom_view::ZoomView;
use crate::{config, settings};

const MIN_WIDTH: i32 = 440;
const MIN_HEIGHT: i32 = 200;
/// Header-, Werkzeug- und Aktionsleiste, für die Startgröße des Fensters.
const CHROME_HEIGHT: i32 = 142;

/// The image with its shapes burned in, keyed by the annotation revision it
/// was rendered from – copying twice doesn't render twice.
type ExportCache = Rc<RefCell<Option<(u64, Rc<Screenshot>)>>>;

thread_local! {
    static CSS_LOADED: Cell<bool> = const { Cell::new(false) };
}

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
    view.set_color(settings::get().annotation_color);
    let (tools_bar, pointer_button) = tools_bar(&view);

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
    layout.add_top_bar(&tools_bar);
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

    install_actions(&window, &toasts, &view, &screenshot, &pointer_button);
    window.present();
}

/// Drawing tools, color palette and undo, above the image.
fn tools_bar(view: &ZoomView) -> (gtk::Box, gtk::ToggleButton) {
    ensure_css(&WidgetExt::display(view));

    // Flat like the palette next to it; only the active tool is highlighted.
    let tools = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    let mut pointer_button: Option<gtk::ToggleButton> = None;
    for (tool, label) in [
        (Tool::Pointer, tr("Pointer: pan and zoom (Esc)")),
        (Tool::Shape(ShapeKind::Rectangle), tr("Rectangle")),
        (Tool::Shape(ShapeKind::Ellipse), tr("Ellipse")),
        (Tool::Shape(ShapeKind::Arrow), tr("Arrow")),
    ] {
        let button = gtk::ToggleButton::builder()
            .child(&ToolIcon::new(tool))
            .tooltip_text(label)
            .css_classes(["flat"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(label)]);
        button.set_group(pointer_button.as_ref());
        button.connect_toggled(glib::clone!(
            #[weak]
            view,
            move |button| {
                if button.is_active() {
                    view.set_tool(tool);
                }
            }
        ));
        tools.append(&button);
        pointer_button.get_or_insert(button);
    }
    let pointer_button = pointer_button.expect("tool list is not empty");
    pointer_button.set_active(true);

    let colors = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    let current = settings::get().annotation_color;
    let mut first_color: Option<gtk::ToggleButton> = None;
    for color in Color::ALL {
        let swatch = gtk::Box::builder()
            .css_classes(["porthole-swatch", &format!("porthole-color-{}", color.name())])
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        let label = color_label(color);
        let button = gtk::ToggleButton::builder()
            .child(&swatch)
            .tooltip_text(label)
            .css_classes(["flat", "circular"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(label)]);
        button.set_group(first_color.as_ref());
        // Before connecting: restoring the choice is not a change to save.
        button.set_active(color == current);
        button.connect_toggled(glib::clone!(
            #[weak]
            view,
            move |button| {
                if button.is_active() {
                    view.set_color(color);
                    settings::update(|settings| settings.annotation_color = color);
                }
            }
        ));
        colors.append(&button);
        first_color.get_or_insert(button);
    }

    let undo = gtk::Button::builder()
        .icon_name("edit-undo-symbolic")
        .action_name("preview.undo")
        .tooltip_text(tr("Undo last shape (Ctrl+Z)"))
        .build();

    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    bar.add_css_class("toolbar");
    bar.append(&tools);
    bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    bar.append(&colors);
    bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    bar.append(&undo);
    (bar, pointer_button)
}

fn color_label(color: Color) -> &'static str {
    match color {
        Color::Red => tr("Red"),
        Color::Orange => tr("Orange"),
        Color::Green => tr("Green"),
        Color::Blue => tr("Blue"),
        Color::White => tr("White"),
        Color::Black => tr("Black"),
    }
}

/// Swatch styles for the palette, registered once per process.
fn ensure_css(display: &gdk::Display) {
    if CSS_LOADED.replace(true) {
        return;
    }
    let mut css = String::from(
        ".porthole-swatch { min-width: 16px; min-height: 16px; border-radius: 9999px; \
         box-shadow: inset 0 0 0 1px alpha(currentColor, 0.3); }\n",
    );
    for color in Color::ALL {
        css.push_str(&format!(
            ".porthole-color-{} {{ background-color: {}; }}\n",
            color.name(),
            color.hex()
        ));
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css);
    gtk::style_context_add_provider_for_display(
        display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// What copy and save deliver: the original if there are no shapes (same
/// bytes as before, no re-encoding), else the image with the shapes.
fn current_image(view: &ZoomView, original: &Rc<Screenshot>, cache: &ExportCache) -> Option<Rc<Screenshot>> {
    let (shapes, revision) = view.annotations();
    if shapes.is_empty() {
        return Some(original.clone());
    }
    if let Some((cached_revision, image)) = cache.borrow().as_ref() {
        if *cached_revision == revision {
            return Some(image.clone());
        }
    }
    let started = Instant::now();
    let image = Rc::new(original.with_annotations(&shapes)?);
    glib::g_message!(
        config::LOG_DOMAIN,
        "Annotations: {} shapes, render + PNG {} ms",
        shapes.len(),
        started.elapsed().as_millis()
    );
    cache.replace(Some((revision, image.clone())));
    Some(image)
}

fn install_actions(
    window: &adw::Window,
    toasts: &adw::ToastOverlay,
    view: &ZoomView,
    screenshot: &Rc<Screenshot>,
    pointer_button: &gtk::ToggleButton,
) {
    let cache = ExportCache::default();

    let copy = gio::ActionEntry::builder("copy")
        .activate(glib::clone!(
            #[weak]
            window,
            #[weak]
            toasts,
            #[weak]
            view,
            #[strong]
            screenshot,
            #[strong]
            cache,
            move |_: &gio::SimpleActionGroup, _, _| {
                let Some(image) = current_image(&view, &screenshot, &cache) else {
                    show_toast(&toasts, tr("The shapes could not be applied"));
                    return;
                };
                let display = WidgetExt::display(&window);
                let message = match clipboard::copy_image(&display, &image) {
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
            #[weak]
            view,
            #[strong]
            screenshot,
            #[strong]
            cache,
            move |_: &gio::SimpleActionGroup, _, _| {
                // Taken now: shapes drawn while the dialog is open don't count.
                let Some(image) = current_image(&view, &screenshot, &cache) else {
                    show_toast(&toasts, tr("The shapes could not be applied"));
                    return;
                };
                glib::MainContext::default().spawn_local(glib::clone!(
                    #[weak]
                    window,
                    #[weak]
                    toasts,
                    async move {
                        match save::save_with_dialog(&window, &image).await {
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

    let undo = gio::ActionEntry::builder("undo")
        .activate(glib::clone!(
            #[weak]
            view,
            move |_: &gio::SimpleActionGroup, _, _| view.undo_annotation()
        ))
        .build();

    let tool_pointer = gio::ActionEntry::builder("tool-pointer")
        .activate(glib::clone!(
            #[weak]
            pointer_button,
            move |_: &gio::SimpleActionGroup, _, _| pointer_button.set_active(true)
        ))
        .build();

    let group = gio::SimpleActionGroup::new();
    group.add_action_entries([copy, save, delete, zoom_fit, zoom_original, undo, tool_pointer]);
    window.insert_action_group("preview", Some(&group));

    // Undo is only available while there is something to undo.
    if let Some(undo) = group.lookup_action("undo").and_downcast::<gio::SimpleAction>() {
        undo.set_enabled(false);
        view.connect_annotations_changed(move |has_shapes| undo.set_enabled(has_shapes));
    }

    let shortcuts = gtk::ShortcutController::new();
    for (trigger, action) in [
        ("<Control>c", "preview.copy"),
        ("<Control>s", "preview.save"),
        ("Delete", "preview.delete"),
        ("<Control>0|<Control>KP_0", "preview.zoom-fit"),
        ("<Control>1|<Control>KP_1", "preview.zoom-original"),
        ("<Control>z", "preview.undo"),
        ("Escape", "preview.tool-pointer"),
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
