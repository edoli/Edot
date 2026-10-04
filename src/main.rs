#![windows_subsystem = "windows"]

mod app;
mod editing;
mod model;
mod startup;
mod tabs;
mod theme;
mod ui;

fn main() {
    app::run();
}
