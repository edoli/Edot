//! Shared light/dark palette and UI typography. Body typography belongs to Settings.
use super::*;
#[cfg(target_os = "windows")]
pub const UI_FONT: &str = "Segoe UI";
#[cfg(target_os = "macos")]
pub const UI_FONT: &str = ".SystemUIFont";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const UI_FONT: &str = "DejaVu Sans";
pub const UI_FONT_SIZE: f32 = 14.;
pub const TAB_FONT_SIZE: f32 = 12.;
pub const TREE_FONT_SIZE: f32 = 12.;

#[derive(Clone, Copy)]
pub struct Colors {
    pub background: Hsla,
    pub foreground: Hsla,
    pub sidebar: Hsla,
    pub bar: Hsla,
    pub border: Hsla,
    pub primary: Hsla,
    pub selection: Hsla,
    pub multi_selection: Hsla,
    pub accent: Hsla,
    pub muted_foreground: Hsla,
    pub destructive: Hsla,
    pub warning: Hsla,
    pub primary_foreground: Hsla,
    pub primary_hover: Hsla,
    pub primary_active: Hsla,
}
impl Colors {
    pub fn for_appearance(dark: bool) -> Self {
        Self {
            background: rgb(if dark { 0x1e2127 } else { 0xffffff }).into(),
            foreground: rgb(if dark { 0xe5e9f0 } else { 0x1f2937 }).into(),
            sidebar: rgb(if dark { 0x181b21 } else { 0xf5f7fa }).into(),
            bar: rgb(if dark { 0x15181d } else { 0xf1f4f8 }).into(),
            border: rgb(if dark { 0x3e4450 } else { 0xdce2ea }).into(),
            primary: rgb(if dark { 0x6ba4ff } else { 0x2674f5 }).into(),
            selection: rgb(if dark { 0x2b4162 } else { 0xddebff }).into(),
            multi_selection: rgb(if dark { 0x472b62 } else { 0xeeddff }).into(),
            accent: rgb(if dark { 0x292e38 } else { 0xe8edf5 }).into(),
            muted_foreground: rgb(if dark { 0xa5afbf } else { 0x657187 }).into(),
            destructive: rgb(if dark { 0xff8589 } else { 0xc42b32 }).into(),
            warning: rgb(if dark { 0xf5bf67 } else { 0x946200 }).into(),
            primary_foreground: rgb(if dark { 0x10151d } else { 0xffffff }).into(),
            primary_hover: rgb(if dark { 0x86b5ff } else { 0x1b63d9 }).into(),
            primary_active: rgb(if dark { 0x5591ed } else { 0x1656bf }).into(),
        }
    }
}
#[derive(Clone)]
pub struct Theme {
    pub colors: Colors,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            colors: Colors::for_appearance(false),
        }
    }
}
impl std::ops::Deref for Theme {
    type Target = Colors;
    fn deref(&self) -> &Colors {
        &self.colors
    }
}
impl Global for Theme {}
impl Theme {
    pub fn update(cx: &mut App, f: impl FnOnce(&mut Self)) {
        cx.update_global::<Self, _>(|theme, _| f(theme));
        cx.refresh_windows();
    }
}
pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}
impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}
