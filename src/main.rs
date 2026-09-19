mod app;
mod capture;
mod config;
mod controller;
mod geometry;
mod monitors;
mod portal;
mod screenshot;
mod services;
mod ui;
mod viewport;

use gtk::glib;
use gtk::prelude::*;

fn main() -> glib::ExitCode {
    app::build().run()
}
