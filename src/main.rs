#![windows_subsystem = "windows"]

mod desktop;
mod platform;
mod startup;
mod widgets;

use edot::{
    datetime, drag, editing, global_search, model, search, selection, session, settings, space,
    storage, tabs, watch,
};

fn main() {
    desktop::run();
}
