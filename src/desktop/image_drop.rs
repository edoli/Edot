//! Windows overrides OLE; other platforms use GPUI's native file-drop events.
#[cfg(windows)]
mod windows;
#[cfg(all(windows, test))]
pub(super) use windows::image_urls;
#[cfg(windows)]
pub(super) use windows::install;
#[cfg(all(windows, test))]
pub(super) use windows::test_target;

#[cfg(not(windows))]
pub(super) fn install(
    _: &gpui::Entity<super::Edot>,
    _: &mut gpui::Window,
    _: &mut gpui::App,
) -> gpui::Result<()> {
    Ok(())
}
