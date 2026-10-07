//! Small application-specific controls implemented directly with GPUI.
use gpui::{prelude::*, *};
use std::rc::Rc;
mod dialog;
pub mod input;
pub mod menu;
pub mod scroll;
mod select;
pub use dialog::{DialogButtonProps, WindowExt};
pub use input::{Input, InputEvent, InputState, Textarea, TextareaState};
pub use menu::{ContextMenuExt, PopupMenuItem};
pub use select::{Select, SelectEvent, SelectState};

pub fn init(cx: &mut App) {
    cx.set_global(Theme::default());
    input::init(cx);
}
pub trait StyledExt: Styled + Sized {
    fn h_flex(self) -> Self {
        self.flex().flex_row().items_center()
    }
    fn v_flex(self) -> Self {
        self.flex().flex_col()
    }
}
impl<T: Styled> StyledExt for T {}

mod button;
mod icon;
mod switch;
pub mod theme;
mod window;
use button::ClickHandler;
pub use button::{Button, Tooltip};
pub use icon::{Assets, Icon, IconName};
pub use switch::Switch;
pub use theme::{ActiveTheme, Theme};
pub use window::open_window;
