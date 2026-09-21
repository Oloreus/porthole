//! Eigener systemd-Scope beim Start aus dem Terminal einer Snap-App.
//!
//! Aus z. B. dem VS-Code-Terminal (Snap) gestartet, landet Porthole im Scope
//! `snap.code.code-….scope`. Das Portal hält den Prozess dann für VS Code,
//! die Shell ordnet die Fenster keiner App zu (kein Dock-Eintrag), und der
//! Screenshot-Erlaubnis-Dialog wird verweigert („Only the focused app …“).
//! Dagegen hilft nur ein eigener Scope: Ohne Portal-Registry (XDP < 1.20)
//! liest das Portal die App-ID aus dem Unit-Namen `app-<App-ID>@….service`.

use std::env;
use std::fs;
use std::io::IsTerminal;
use std::process::Command;

use gtk::glib;

use crate::config;

/// Verhindert eine Endlosschleife, falls der neue Scope doch wieder als
/// Snap-Scope erscheint.
const GUARD_VAR: &str = "PORTHOLE_OWN_SCOPE";

/// Startet Porthole als eigene systemd-User-Unit neu, wenn der Prozess im
/// Scope einer Snap-App läuft. `Some(code)`: der Neustart ist gelaufen und
/// dies ist sein Exit-Code; `None`: normal weitermachen.
pub fn relaunch_if_in_snap_scope() -> Option<glib::ExitCode> {
    if env::var_os(GUARD_VAR).is_some() {
        return None;
    }
    let scope = snap_scope()?;
    let exe = env::current_exe().ok()?;
    glib::g_message!(
        config::LOG_DOMAIN,
        "Läuft im Scope „{scope}“ – starte in eigener Unit neu"
    );

    let mut command = Command::new("systemd-run");
    command
        .args(["--user", "--quiet", "--collect"])
        .arg(format!("--unit=app-{}@{}", config::APP_ID, std::process::id()))
        .arg(format!("--setenv={GUARD_VAR}=1"));
    // The new unit gets the session's environment, not the terminal's; keep
    // the terminal's language so messages match the caller's locale.
    for var in ["LANG", "LANGUAGE", "LC_ALL", "LC_MESSAGES"] {
        if let Some(value) = env::var_os(var) {
            let mut setenv = std::ffi::OsString::from(format!("--setenv={var}="));
            setenv.push(value);
            command.arg(setenv);
        }
    }
    // Mit Terminal: --pty reicht Strg+C an Porthole durch. Ohne Terminal
    // bliebe Porthole nach Strg+C auf systemd-run weiterlaufen.
    if std::io::stdin().is_terminal() {
        command.arg("--pty");
    } else {
        command.args(["--pipe", "--wait"]);
    }
    command.arg("--").arg(exe).args(env::args_os().skip(1));

    match command.status() {
        Ok(status) => Some(glib::ExitCode::from(
            status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1),
        )),
        Err(err) => {
            glib::g_warning!(
                config::LOG_DOMAIN,
                "systemd-run nicht ausführbar: {err} – Screenshot-Freigabe und \
                 Dock-Eintrag werden scheitern. Über das App-Menü starten."
            );
            None
        }
    }
}

/// Name der eigenen cgroup-Unit, falls sie zu einer Snap-App gehört.
fn snap_scope() -> Option<String> {
    let cgroup = fs::read_to_string("/proc/self/cgroup").ok()?;
    // cgroup v2: eine Zeile "0::/user.slice/…/app.slice/<unit>"
    let path = cgroup.lines().find_map(|line| line.strip_prefix("0::"))?;
    let unit = path.rsplit('/').next()?;
    unit.starts_with("snap.").then(|| unit.to_owned())
}
