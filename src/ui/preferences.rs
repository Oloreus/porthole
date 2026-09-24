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
    page
}
