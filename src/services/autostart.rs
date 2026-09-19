//! Autostart über einen XDG-Autostart-Eintrag (`~/.config/autostart`).
//! Der Dateiname entspricht der App-ID, damit GNOME den Prozess beim Login
//! der App zuordnet. Reine Dateioperationen – aus jedem Thread nutzbar.

use std::path::PathBuf;
use std::{env, fs, io};

use crate::config;

fn entry_path() -> Option<PathBuf> {
    let config_dir = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config_dir.join("autostart").join(format!("{}.desktop", config::APP_ID)))
}

pub fn is_enabled() -> bool {
    entry_path().is_some_and(|path| path.exists())
}

pub fn set_enabled(enabled: bool) -> io::Result<()> {
    let path = entry_path().ok_or_else(|| io::Error::other("Kein Konfigurationsverzeichnis"))?;
    if !enabled {
        return match fs::remove_file(&path) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        };
    }

    let executable = env::current_exe()?;
    let entry = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Exec=\"{exec}\" --background\n\
         Icon={id}\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n",
        name = config::APP_NAME,
        exec = executable.display(),
        id = config::APP_ID,
    );
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, entry)
}
