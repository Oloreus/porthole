//! User preferences, stored as a key file in `~/.config/porthole/settings.ini`.
//! No GSettings: that would need a compiled schema installed system-wide.
//! Main thread only; loaded on first access and written on every change.

use std::cell::{Cell, OnceCell};
use std::path::PathBuf;

use gtk::glib;

use crate::annotations::Color;
use crate::config;

const GROUP_PREVIEW: &str = "preview";
const KEY_CLOSE_AFTER_COPY: &str = "close-after-copy";
const GROUP_ANNOTATE: &str = "annotate";
const KEY_COLOR: &str = "color";

#[derive(Clone, Copy, Debug, Default)]
pub struct Settings {
    /// Close the preview window once its image is on the clipboard.
    pub close_preview_after_copy: bool,
    /// Last color picked in the screenshot window's palette.
    pub annotation_color: Color,
}

thread_local! {
    static CURRENT: OnceCell<Cell<Settings>> = const { OnceCell::new() };
}

pub fn get() -> Settings {
    CURRENT.with(|current| current.get_or_init(|| Cell::new(load())).get())
}

pub fn update(change: impl FnOnce(&mut Settings)) {
    let mut settings = get();
    change(&mut settings);
    CURRENT.with(|current| current.get_or_init(|| Cell::new(settings)).set(settings));
    if let Err(err) = save(&settings) {
        glib::g_warning!(config::LOG_DOMAIN, "Settings not saved: {err}");
    }
}

fn path() -> PathBuf {
    glib::user_config_dir().join("porthole").join("settings.ini")
}

fn load() -> Settings {
    let file = glib::KeyFile::new();
    let defaults = Settings::default();
    // Missing file or keys: keep the defaults.
    if file.load_from_file(path(), glib::KeyFileFlags::NONE).is_err() {
        return defaults;
    }
    Settings {
        close_preview_after_copy: file
            .boolean(GROUP_PREVIEW, KEY_CLOSE_AFTER_COPY)
            .unwrap_or(defaults.close_preview_after_copy),
        annotation_color: file
            .string(GROUP_ANNOTATE, KEY_COLOR)
            .ok()
            .and_then(|name| Color::from_name(&name))
            .unwrap_or(defaults.annotation_color),
    }
}

fn save(settings: &Settings) -> Result<(), glib::Error> {
    let file = glib::KeyFile::new();
    file.set_boolean(GROUP_PREVIEW, KEY_CLOSE_AFTER_COPY, settings.close_preview_after_copy);
    file.set_string(GROUP_ANNOTATE, KEY_COLOR, settings.annotation_color.name());

    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|err| glib::Error::new(glib::FileError::Failed, &err.to_string()))?;
    }
    file.save_to_file(path)
}
