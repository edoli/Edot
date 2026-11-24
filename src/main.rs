#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod desktop;
mod error_log;
mod image_http;
#[cfg(target_os = "windows")]
mod platform;
mod startup;
mod update;
mod widgets;

use edot::{
    datetime, drag, editing, global_search, model, search, selection, session, settings, shortcuts,
    space, storage, store_lock, tabs, trash, watch,
};

fn main() {
    error_log::install();
    desktop::run();
}
