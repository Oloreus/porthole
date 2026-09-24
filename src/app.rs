use adw::prelude::*;
use gtk::{gio, glib};

use crate::capture::permission;
use crate::controller::CaptureController;
use crate::services::{shortcuts, tray};
use crate::{config, i18n, portal, ui};

const USAGE: &str = "\
Usage: porthole [OPTION]
  (no option)          Show the window
  --capture            Take a region screenshot
  --background         Start in the background only, no window
  --reset-permission   Delete the stored screenshot permission (you will be asked again)
";

/// Single-instance application. Every further invocation (including
/// `porthole --capture` from a desktop shortcut) reaches `on_command_line` of
/// the running instance via D-Bus; all triggers end up in the `app.capture` action.
pub fn build() -> adw::Application {
    let app = adw::Application::builder()
        .application_id(config::APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE | gio::ApplicationFlags::SEND_ENVIRONMENT)
        .build();

    app.connect_startup(on_startup);
    app.connect_activate(on_activate);
    app.connect_command_line(on_command_line);
    app
}

fn on_startup(app: &adw::Application) {
    // Keeps running in the background without a window until `app.quit`.
    std::mem::forget(app.hold());

    // Registration must finish before any other portal call, otherwise the
    // app ID counts as empty and BindShortcuts fails.
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
    // From the camera button in our own window: get the window out of the shot first.
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
                    Ok(()) => glib::g_message!(config::LOG_DOMAIN, "Screenshot permission reset"),
                    Err(err) => glib::g_warning!(config::LOG_DOMAIN, "Reset failed: {err}"),
                }
            });
        })
        .build();
    let open_keyboard_settings = gio::ActionEntry::builder("open-keyboard-settings")
        .activate(|_: &adw::Application, _, _| shortcuts::open_keyboard_settings())
        .build();
    let preferences = gio::ActionEntry::builder("preferences")
        .activate(|app: &adw::Application, _, _| {
            // The dialog needs a parent window.
            ui::main_window::present(app);
            if let Some(window) = ui::main_window::window() {
                ui::preferences::present(&window);
            }
        })
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
        preferences,
        quit,
    ]);
    app.set_accels_for_action("app.preferences", &["<Control>comma"]);
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

    let usage = usage_text(cmdline);
    match args.as_slice() {
        [] => app.activate(),
        [flag] if flag == "--capture" => app.activate_action("capture", None),
        [flag] if flag == "--reset-permission" => app.activate_action("reset-permission", None),
        [flag] if flag == "--background" => {}
        [flag] if flag == "--help" || flag == "-h" => cmdline.print_literal(&usage),
        _ => {
            cmdline.printerr_literal(&usage);
            return glib::ExitCode::FAILURE;
        }
    }
    glib::ExitCode::SUCCESS
}

/// Usage text in the caller's language. The running instance may have been
/// started with a different locale, so read it from the calling process.
fn usage_text(cmdline: &gio::ApplicationCommandLine) -> String {
    match i18n::locale_from(|var| cmdline.getenv(var).map(String::from)) {
        Some(locale) => i18n::tr_for(&locale, USAGE),
        None => USAGE.to_owned(),
    }
}
