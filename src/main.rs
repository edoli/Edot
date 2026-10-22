#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod desktop;
mod image_http;
#[cfg(target_os = "windows")]
mod platform;
mod startup;
mod update;
mod widgets;

use edot::{
    datetime, drag, editing, global_search, model, search, selection, session, settings, space,
    storage, store_lock, tabs, trash, watch,
};

fn main() {
    desktop::run();
}
