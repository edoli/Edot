//! Shared application layout (logical pixels) and interaction timing.
use crate::session::{MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH};
use std::time::Duration;

pub(super) const SIDEBAR_RESIZE_WIDTH: f32 = 6.;
pub(super) const EDITOR_MIN_WIDTH: f32 = 220.;
pub(super) const TAB_MIN_WIDTH: i32 = 100;
pub(super) const TAB_PREFERRED_WIDTH: i32 = 180;
pub(super) const TREE_ROW_HEIGHT: f32 = 29.;
pub(super) const TREE_INDENT: f32 = 22.;
// Row padding + disclosure width + gap: insertion lines start at the note icon.
pub(super) const TREE_ICON_LEFT: f32 = 24.;
pub(super) const SEARCH_ROW_HEIGHT: f32 = 26.;
pub(super) const SEARCH_INPUT_FONT_SIZE: f32 = 14.;
pub(super) const TREE_SCROLL_EDGE: f32 = 24.;
pub(super) const TREE_SCROLL_ROWS: f32 = 2.;
pub(super) const TREE_SCROLL_INTERVAL: Duration = Duration::from_millis(160);
pub(super) const AUTOSAVE_DELAY: Duration = Duration::from_millis(650);
pub(super) const SEARCH_DELAY: Duration = Duration::from_millis(180);
pub(super) const ZOOM_DEFAULT: u32 = 100;
pub(super) const ZOOM_MIN: u32 = 10;
pub(super) const ZOOM_MAX: u32 = 500;
pub(super) const ZOOM_STEP: u32 = 10;

pub(super) fn sidebar_width(configured: i32, viewport: f32) -> f32 {
    (configured as f32)
        .min(viewport - EDITOR_MIN_WIDTH)
        .max(MIN_SIDEBAR_WIDTH as f32)
}
pub(super) fn sidebar_max_width(viewport: f32) -> i32 {
    (viewport as i32 - EDITOR_MIN_WIDTH as i32).clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
}
