use std::cell::RefCell;
use std::path::PathBuf;

use gtk::prelude::*;
use gtk::{gio, glib};

use crate::screenshot::Screenshot;

thread_local! {
    /// Zuletzt gewählter Ordner (bis zum Beenden der App).
    static LAST_FOLDER: RefCell<Option<gio::File>> = const { RefCell::new(None) };
}

pub enum SaveOutcome {
    Saved(PathBuf),
    Dismissed,
}

/// Nativer Speichern-Dialog mit Namensvorschlag; schreibt die PNG-Bytes.
pub async fn save_with_dialog(
    parent: &impl IsA<gtk::Window>,
    screenshot: &Screenshot,
) -> Result<SaveOutcome, String> {
    let png_filter = gtk::FileFilter::new();
    png_filter.set_name(Some("PNG-Bild"));
    png_filter.add_mime_type("image/png");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&png_filter);

    let folder = LAST_FOLDER.with_borrow(Clone::clone).or_else(|| {
        glib::user_special_dir(glib::UserDirectory::Pictures).map(gio::File::for_path)
    });

    let mut dialog = gtk::FileDialog::builder()
        .title("Screenshot speichern")
        .modal(true)
        .initial_name(screenshot.suggested_filename())
        .filters(&filters)
        .default_filter(&png_filter);
    if let Some(folder) = &folder {
        dialog = dialog.initial_folder(folder);
    }

    let file = match dialog.build().save_future(Some(parent)).await {
        Ok(file) => file,
        Err(err) if err.matches(gtk::DialogError::Dismissed) => return Ok(SaveOutcome::Dismissed),
        Err(err) => return Err(err.to_string()),
    };
    let file = with_png_extension(file);

    file.replace_contents_future(
        screenshot.png.clone(),
        None,
        false,
        gio::FileCreateFlags::REPLACE_DESTINATION,
    )
    .await
    .map_err(|(_, err)| err.to_string())?;

    LAST_FOLDER.set(file.parent());
    Ok(SaveOutcome::Saved(file.path().unwrap_or_default()))
}

fn with_png_extension(file: gio::File) -> gio::File {
    match file.path() {
        Some(path) if path.extension().is_none() => {
            gio::File::for_path(path.with_extension("png"))
        }
        _ => file,
    }
}
