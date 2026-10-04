#![windows_subsystem = "windows"]

mod app;
mod drag;
mod editing;
mod model;
mod settings;
mod startup;
mod tabs;
mod theme;
mod tooltip;
mod ui;

fn main() {
    app::run();
}
