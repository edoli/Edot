//! Edot's plain-text editor. Byte offsets internally, UTF-16 only at the IME boundary.
use super::*;
use std::{ops::Range, time::Duration};

const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(530);
use unicode_segmentation::UnicodeSegmentation;

mod commands;
pub use commands::{Copy, Cut, EDIT_COMMANDS, Paste, Redo, SelectAll, Undo, init};

pub enum InputEvent {
    Change,
    PressEnter { shift: bool },
    Blur,
}
#[derive(Clone)]
struct Edit {
    start: usize,
    before: String,
    after: String,
    selection: Range<usize>,
    reversed: bool,
    after_wrap: bool,
    extra: Vec<Selection>,
    others: Vec<Replacement>,
}
#[derive(Clone)]
struct Selection {
    range: Range<usize>,
    reversed: bool,
    preferred_x: Option<Pixels>,
    after_wrap: bool,
}
struct Composition {
    before: SharedString,
    primary: Selection,
    extra: Vec<Selection>,
}
#[derive(Clone)]
struct Replacement {
    start: usize,
    before: String,
    after: String,
}
struct Paragraph {
    start: usize,
    end: usize,
    top: Pixels,
    line: WrappedLine,
    tabs: Vec<(usize, usize, usize)>,
    links: Vec<Range<usize>>,
}
pub struct InputState {
    focus: FocusHandle,
    content: SharedString,
    pub revision: u64,
    placeholder: SharedString,
    label: SharedString,
    selection: Range<usize>,
    reversed: bool,
    caret_after_wrap: bool,
    extra: Vec<Selection>,
    occurrence_word: bool,
    marked: Option<Range<usize>>,
    composition: Option<Composition>,
    extra_marked: Vec<Range<usize>>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    last_edit: Option<std::time::Instant>,
    focused: bool,
    scroll_drag: Option<(bool, Pixels, Pixels)>,
    scroll_hover: Option<bool>,
    link_hover: bool,
    wheel_gesture: OngoingScroll,
    multiline: bool,
    wrap: bool,
    scroll: Point<Pixels>,
    bounds: Bounds<Pixels>,
    content_padding: Edges<Pixels>,
    lines: Vec<Paragraph>,
    layout_key: Option<(u64, Pixels, Pixels, Font, Hsla, Hsla)>,
    line_height: Pixels,
    content_size: Size<Pixels>,
    selecting: bool,
    column_anchor: Option<Point<Pixels>>,
    reveal: bool,
    preferred_x: Option<Pixels>,
    blink: bool,
    _blink_task: Task<()>,
    _blur: Subscription,
}
pub type TextareaState = InputState;
impl EventEmitter<InputEvent> for InputState {}
impl Focusable for InputState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl InputState {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle().tab_stop(true);
        let blur = cx.on_blur(&focus, window, |s, _, cx| {
            s.selecting = false;
            s.column_anchor = None;
            s.focused = false;
            cx.emit(InputEvent::Blur);
            cx.notify();
        });
        cx.on_focus(&focus, window, |s, _, cx| {
            s.focused = true;
            s.blink = true;
            cx.notify();
        })
        .detach();
        let blink_task = cx.spawn(async |entity, cx| {
            loop {
                cx.background_executor().timer(CARET_BLINK_INTERVAL).await;
                if entity
                    .update(cx, |s, cx| {
                        if s.focused {
                            s.blink = !s.blink;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            focus,
            content: "".into(),
            revision: 0,
            placeholder: "".into(),
            label: "".into(),
            selection: 0..0,
            reversed: false,
            caret_after_wrap: false,
            extra: vec![],
            occurrence_word: false,
            marked: None,
            composition: None,
            extra_marked: vec![],
            undo: vec![],
            redo: vec![],
            last_edit: None,
            focused: false,
            scroll_drag: None,
            scroll_hover: None,
            link_hover: false,
            wheel_gesture: OngoingScroll::default(),
            multiline: false,
            wrap: false,
            scroll: point(px(0.), px(0.)),
            bounds: Bounds::default(),
            content_padding: Edges::default(),
            lines: vec![],
            layout_key: None,
            line_height: px(24.),
            content_size: Size::default(),
            selecting: false,
            column_anchor: None,
            reveal: false,
            preferred_x: None,
            blink: true,
            _blink_task: blink_task,
            _blur: blur,
        }
    }
    pub fn placeholder(mut self, value: impl Into<SharedString>) -> Self {
        self.placeholder = value.into();
        self
    }
    pub fn soft_wrap(mut self, value: bool) -> Self {
        self.multiline = true;
        self.wrap = value;
        self
    }

    pub fn content_padding(mut self, padding: Edges<Pixels>) -> Self {
        self.content_padding = padding;
        self
    }
    pub fn default_value(mut self, text: String) -> Self {
        self.content = text.into();
        self
    }
    pub fn value(&self) -> SharedString {
        self.content.clone()
    }
    pub fn selected_value(&self) -> &str {
        &self.content[self.selection.clone()]
    }
    pub fn selected_range(&self) -> Range<usize> {
        self.selection.clone()
    }
    pub fn selected_ranges(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        self.extra
            .iter()
            .map(|s| s.range.clone())
            .chain(std::iter::once(self.selection.clone()))
    }
    pub fn cursor(&self) -> usize {
        if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.focus.focus(window, cx);
    }
    pub fn scroll_offset(&self) -> Point<Pixels> {
        self.scroll
    }
    pub fn set_scroll_offset(&mut self, p: Point<Pixels>, cx: &mut Context<Self>) {
        self.scroll = p;
        cx.notify();
    }
    pub fn set_soft_wrap(&mut self, wrap: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.wrap = wrap;
        self.layout_key = None;
        cx.notify();
    }
    pub fn set_value(
        &mut self,
        value: impl Into<SharedString>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.content = value.into();
        self.selection = 0..0;
        self.clear_extra_selections();
        self.reversed = false;
        self.marked = None;
        self.composition = None;
        self.undo.clear();
        self.redo.clear();
        self.last_edit = None;
        self.scroll = point(px(0.), px(0.));
        self.changed(cx);
    }
    fn boundary(&self, n: usize) -> usize {
        let mut n = n.min(self.content.len());
        while !self.content.is_char_boundary(n) {
            n -= 1;
        }
        n
    }
    pub fn set_selected_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.clear_extra_selections();
        self.last_edit = None;
        self.selection = self.boundary(range.start)..self.boundary(range.end.max(range.start));
        self.reversed = false;
        self.moved(cx);
    }
    pub fn select_all(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_extra_selections();
        self.last_edit = None;
        self.selection = 0..self.content.len();
        self.reversed = false;
        self.moved(cx);
    }
    pub fn replace(&mut self, text: impl AsRef<str>, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, text.as_ref(), window, cx);
    }
    pub fn replace_all(&mut self, text: impl AsRef<str>, _: &mut Window, cx: &mut Context<Self>) {
        self.commit(0..self.content.len(), text.as_ref(), false, cx);
    }
    pub fn is_composing(&self) -> bool {
        self.composition.is_some() || self.marked.is_some()
    }
    /// Keep the editor entity, undo history, selections and scroll when a clean
    /// document changes on disk. External reload itself is one undoable edit.
    pub fn reload_from_disk(&mut self, text: &str, cx: &mut Context<Self>) {
        if text == self.content.as_ref() {
            return;
        }
        let selection = self.selection.clone();
        let reversed = self.reversed;
        let extra = self.extra.clone();
        let scroll = self.scroll;
        self.last_edit = None;
        self.commit(0..self.content.len(), text, false, cx);
        self.selection = self.boundary(selection.start)..self.boundary(selection.end);
        self.reversed = reversed;
        self.extra = extra
            .into_iter()
            .map(|mut selection| {
                selection.range =
                    self.boundary(selection.range.start)..self.boundary(selection.range.end);
                selection
            })
            .collect();
        self.deduplicate_selections();
        self.scroll = scroll;
        self.reveal = false;
        self.last_edit = None;
        cx.notify();
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.column_anchor = None;
        self.selecting = false;
        self.revision = self.revision.wrapping_add(1);
        self.layout_key = None;
        self.moved(cx);
        cx.emit(InputEvent::Change);
    }
    fn moved(&mut self, cx: &mut Context<Self>) {
        self.blink = true;
        self.reveal = true;
        cx.notify();
    }
}

mod column;
mod element;
mod history;
mod ime;
mod layout;
mod links;
mod navigation;
mod selection;
pub use element::{Input, Textarea};
