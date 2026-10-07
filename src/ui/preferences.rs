use adw::prelude::*;
use gtk::glib;

use crate::i18n::tr;
use crate::services::autostart;
use crate::{config, settings};

/// The preferences, shown as the content of the main window. Each setting is
/// a row that writes through to `settings` immediately; new settings go into
/// a fitting group here.
pub fn page() -> adw::PreferencesPage {
    let current = settings::get();

    // Not in `settings`: the autostart entry itself is the setting.
    let autostart_row = adw::SwitchRow::builder()
        .title(tr("Start at login"))
        .active(autostart::is_enabled())
        .build();
    autostart_row.connect_active_notify(|row| {
        if let Err(err) = autostart::set_enabled(row.is_active()) {
            glib::g_warning!(config::LOG_DOMAIN, "Autostart not changeable: {err}");
            row.set_active(autostart::is_enabled());
        }
    });

    let general_group = adw::PreferencesGroup::builder()
        .title(tr("General"))
        .build();
    general_group.add(&autostart_row);

    let close_after_copy = adw::SwitchRow::builder()
        .title(tr("Close after copying"))
        .subtitle(tr("Applies to the Copy button and Ctrl+C"))
        .active(current.close_preview_after_copy)
        .build();
    close_after_copy.connect_active_notify(|row| {
        let active = row.is_active();
        settings::update(|settings| settings.close_preview_after_copy = active);
    });

    let preview_group = adw::PreferencesGroup::builder()
        .title(tr("Screenshot Window"))
        .build();
    preview_group.add(&close_after_copy);

    let page = adw::PreferencesPage::new();
    page.add(&general_group);
    page.add(&preview_group);
    for group in shortcut_groups() {
        page.add(&group);
    }
    page
}

/// How a shortcut is triggered: GTK accelerators (shown as keycaps) or a
/// mouse gesture described in words.
enum Trigger {
    Keys(&'static str),
    Mouse(&'static str),
}

/// Read-only overview of all shortcuts. Keep in sync with the handlers in
/// `services::shortcuts`, `ui::overlay`, `ui::preview` and `ui::zoom_view`.
fn shortcut_groups() -> Vec<adw::PreferencesGroup> {
    use Trigger::{Keys, Mouse};

    let groups: [(&str, Option<&str>, &[(&str, Trigger)]); 4] = [
        (
            "Global Shortcut",
            Some("Default; it can be changed in the GNOME Settings under Apps → Porthole"),
            &[("Take region screenshot", Keys("<Alt>s"))],
        ),
        (
            "Selecting an Area",
            None,
            &[
                ("Select area", Mouse("Drag")),
                ("Cancel", Keys("Escape")),
                ("Cancel", Mouse("Right-click")),
            ],
        ),
        (
            "Screenshot Window Shortcuts",
            None,
            &[
                ("Copy to clipboard", Keys("<Control>c")),
                ("Save as PNG", Keys("<Control>s")),
                ("Discard screenshot", Keys("Delete")),
                ("Undo last annotation", Keys("<Control>z")),
                ("Switch to the pointer", Keys("Escape")),
                ("Fit to window", Keys("<Control>0")),
                ("Original size (100 %)", Keys("<Control>1")),
                ("Toggle fit and 100 %", Mouse("Double-click")),
                ("Zoom", Mouse("Ctrl + scroll wheel")),
                ("Pan", Mouse("Scroll wheel, Shift + scroll wheel")),
                ("Pan", Mouse("Drag with the pointer or the middle mouse button")),
            ],
        ),
        (
            "Typing Text",
            None,
            &[
                ("Place text", Mouse("Click with the text tool")),
                ("Finish text", Keys("Return")),
                ("New line", Keys("<Shift>Return")),
                ("Delete last character", Keys("BackSpace")),
                ("Discard text", Keys("Escape")),
            ],
        ),
    ];

    groups
        .into_iter()
        .map(|(title, description, rows)| {
            let group = adw::PreferencesGroup::builder().title(tr(title)).build();
            if let Some(description) = description {
                group.set_description(Some(tr(description)));
            }
            for (title, trigger) in rows {
                let row = adw::ActionRow::builder().title(tr(title)).build();
                let suffix: gtk::Widget = match trigger {
                    Keys(accelerator) => gtk::ShortcutLabel::builder()
                        .accelerator(*accelerator)
                        .valign(gtk::Align::Center)
                        .build()
                        .upcast(),
                    Mouse(text) => gtk::Label::builder()
                        .label(tr(text))
                        .wrap(true)
                        .xalign(1.0)
                        .css_classes(["dim-label"])
                        .build()
                        .upcast(),
                };
                row.add_suffix(&suffix);
                group.add(&row);
            }
            group
        })
        .collect()
}
