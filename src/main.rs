mod app;
mod capture;
mod config;
mod controller;
mod geometry;
mod i18n;
mod monitors;
mod portal;
mod scope;
mod screenshot;
mod services;
mod ui;
mod viewport;

use gtk::glib;
use gtk::prelude::*;

fn main() -> glib::ExitCode {
    if let Some(code) = scope::relaunch_if_in_snap_scope() {
        return code;
    }
    app::build().run()
}
