#![windows_subsystem = "windows"]

mod app;
mod datetime;
mod drag;
mod editing;
mod global_search;
mod model;
mod scrollbar;
mod selection;
mod settings;
mod settings_controls;
mod startup;
mod tabs;
mod theme;
mod tooltip;
mod ui;

fn main() {
    app::run();
}
