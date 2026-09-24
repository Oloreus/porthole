use adw::prelude::*;

use crate::i18n::tr;
use crate::settings;

/// Preferences dialog. Each setting is a row that writes through to
/// `settings` immediately; new settings go into a fitting group here.
pub fn present(parent: &impl IsA<gtk::Widget>) {
    let current = settings::get();

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
    page.add(&preview_group);

    let dialog = adw::PreferencesDialog::builder()
        .title(tr("Preferences"))
        .build();
    dialog.add(&page);
    dialog.present(Some(parent));
}
