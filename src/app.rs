use adw::prelude::*;
use gtk::{gio, glib};

use crate::capture::permission;
use crate::controller::CaptureController;
use crate::services::{shortcuts, tray};
use crate::{config, portal, ui};

const USAGE: &str = "\
Aufruf: porthole [OPTION]
  (ohne Option)        Fenster anzeigen
  --capture            Bereichs-Screenshot starten
  --background         Nur im Hintergrund starten, kein Fenster
  --reset-permission   Gespeicherte Screenshot-Berechtigung löschen (es wird neu gefragt)
";

/// Single-Instance-Anwendung. Jeder weitere Aufruf (auch `porthole --capture`
/// aus einem Desktop-Tastenkürzel) landet per D-Bus in `on_command_line` der
/// laufenden Instanz; alle Auslöser münden in die Action `app.capture`.
pub fn build() -> adw::Application {
    let app = adw::Application::builder()
        .application_id(config::APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    app.connect_startup(on_startup);
    app.connect_activate(on_activate);
    app.connect_command_line(on_command_line);
    app
}

fn on_startup(app: &adw::Application) {
    // Läuft ohne Fenster im Hintergrund weiter, bis `app.quit`.
    std::mem::forget(app.hold());

    // Aus dem Terminal einer Snap-App (z. B. VS Code) gestartet, erbt der
    // Prozess deren Umgebung und Scope; GNOME ordnet das Fenster dann nicht
    // Porthole zu und verweigert den Erlaubnis-Dialog.
    if let Some(snap) = std::env::var_os("SNAP_NAME") {
        glib::g_warning!(
            config::LOG_DOMAIN,
            "Läuft in der Snap-Umgebung von „{}“ – Screenshot-Freigabe wird \
             scheitern. Über das App-Menü oder `systemd-run --user` starten (siehe README).",
            snap.to_string_lossy()
        );
    }

    // Die Registrierung muss vor jedem anderen Portal-Aufruf durch sein,
    // sonst gilt die App-ID als leer und BindShortcuts scheitert.
    glib::MainContext::default().spawn_local(glib::clone!(
        #[strong]
        app,
        async move {
            portal::register_host_app().await;
            shortcuts::run(app).await;
        }
    ));

    tray::start();

    let controller = CaptureController::new(app);
    let capture = gio::ActionEntry::builder("capture")
        .activate(glib::clone!(
            #[strong]
            controller,
            move |_: &adw::Application, _, _| controller.request_capture(false)
        ))
        .build();
    // Vom Kamera-Button des eigenen Fensters: Fenster vorher aus dem Bild nehmen.
    let capture_from_window = gio::ActionEntry::builder("capture-from-window")
        .activate(move |_: &adw::Application, _, _| controller.request_capture(true))
        .build();
    let show = gio::ActionEntry::builder("show")
        .activate(|app: &adw::Application, _, _| app.activate())
        .build();
    let reset_permission = gio::ActionEntry::builder("reset-permission")
        .activate(|_: &adw::Application, _, _| {
            glib::MainContext::default().spawn_local(async {
                match permission::reset().await {
                    Ok(()) => glib::g_message!(
                        config::LOG_DOMAIN,
                        "Screenshot-Berechtigung zurückgesetzt"
                    ),
                    Err(err) => glib::g_warning!(
                        config::LOG_DOMAIN,
                        "Zurücksetzen fehlgeschlagen: {err}"
                    ),
                }
            });
        })
        .build();
    let open_keyboard_settings = gio::ActionEntry::builder("open-keyboard-settings")
        .activate(|_: &adw::Application, _, _| shortcuts::open_keyboard_settings())
        .build();
    let quit = gio::ActionEntry::builder("quit")
        .activate(|app: &adw::Application, _, _| app.quit())
        .build();
    app.add_action_entries([
        capture,
        capture_from_window,
        show,
        reset_permission,
        open_keyboard_settings,
        quit,
    ]);
}

fn on_activate(app: &adw::Application) {
    ui::main_window::present(app);
}

fn on_command_line(app: &adw::Application, cmdline: &gio::ApplicationCommandLine) -> glib::ExitCode {
    let args: Vec<String> = cmdline
        .arguments()
        .iter()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

    match args.as_slice() {
        [] => app.activate(),
        [flag] if flag == "--capture" => app.activate_action("capture", None),
        [flag] if flag == "--reset-permission" => app.activate_action("reset-permission", None),
        [flag] if flag == "--background" => {}
        [flag] if flag == "--help" || flag == "-h" => cmdline.print_literal(USAGE),
        _ => {
            cmdline.printerr_literal(USAGE);
            return glib::ExitCode::FAILURE;
        }
    }
    glib::ExitCode::SUCCESS
}
