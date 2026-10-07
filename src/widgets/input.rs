//! Edot's plain-text editor. Byte offsets internally, UTF-16 only at the IME boundary.
use super::*;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

actions!(edot_input, [Undo, Redo, Cut, Copy, Paste, SelectAll]);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-z", Undo, Some("EdotInput")),
        KeyBinding::new("ctrl-y", Redo, Some("EdotInput")),
        KeyBinding::new("ctrl-shift-z", Redo, Some("EdotInput")),
        KeyBinding::new("ctrl-x", Cut, Some("EdotInput")),
        KeyBinding::new("ctrl-c", Copy, Some("EdotInput")),
        KeyBinding::new("ctrl-v", Paste, Some("EdotInput")),
        KeyBinding::new("ctrl-a", SelectAll, Some("EdotInput")),
    ]);
}

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
}
struct Paragraph {
    start: usize,
    end: usize,
    top: Pixels,
    line: WrappedLine,
    tabs: Vec<(usize, usize, usize)>,
}
impl Paragraph {
    fn position(&self, byte: usize, height: Pixels) -> Point<Pixels> {
        let added: usize = self
            .tabs
            .iter()
            .filter(|(source, _, _)| *source < byte)
            .map(|(_, _, spaces)| spaces - 1)
            .sum();
        self.line
            .position_for_index(byte + added, height)
            .unwrap_or_default()
    }
    fn source_index(&self, display: usize) -> usize {
        let mut removed = 0;
        for &(source, start, spaces) in &self.tabs {
            if display < start {
                break;
            }
            if display < start + spaces {
                return source + usize::from(display - start >= spaces / 2);
            }
            removed += spaces - 1;
        }
        display.saturating_sub(removed).min(self.end - self.start)
    }
}
fn expand_tabs(text: &str) -> (SharedString, Vec<(usize, usize, usize)>) {
    if !text.contains('\t') {
        return (text.to_owned().into(), vec![]);
    }
    let mut display = String::new();
    let mut tabs = vec![];
    let mut column = 0;
    for (byte, ch) in text.char_indices() {
        if ch == '\t' {
            let spaces = 4 - column % 4;
            tabs.push((byte, display.len(), spaces));
            display.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            display.push(ch);
            column += 1;
        }
    }
    (display.into(), tabs)
}

pub struct InputState {
    focus: FocusHandle,
    content: SharedString,
    pub revision: u64,
    placeholder: SharedString,
    label: SharedString,
    selection: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    composition: Option<(SharedString, Range<usize>, bool)>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    last_edit: Option<std::time::Instant>,
    focused: bool,
    scroll_drag: Option<(bool, Pixels, Pixels)>,
    scroll_hover: Option<bool>,
    wheel_gesture: OngoingScroll,
    multiline: bool,
    wrap: bool,
    scroll: Point<Pixels>,
    bounds: Bounds<Pixels>,
    content_padding: Edges<Pixels>,
    #[cfg(test)]
    paint_clip: Bounds<Pixels>,
    lines: Vec<Paragraph>,
    layout_key: Option<(u64, Pixels, Pixels, Font, Hsla)>,
    line_height: Pixels,
    content_size: Size<Pixels>,
    selecting: bool,
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
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(530))
                    .await;
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
            marked: None,
            composition: None,
            undo: vec![],
            redo: vec![],
            last_edit: None,
            focused: false,
            scroll_drag: None,
            scroll_hover: None,
            wheel_gesture: OngoingScroll::default(),
            multiline: false,
            wrap: false,
            scroll: point(px(0.), px(0.)),
            bounds: Bounds::default(),
            content_padding: Edges::default(),
            #[cfg(test)]
            paint_clip: Bounds::default(),
            lines: vec![],
            layout_key: None,
            line_height: px(24.),
            content_size: Size::default(),
            selecting: false,
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
        self.last_edit = None;
        self.selection = self.boundary(range.start)..self.boundary(range.end.max(range.start));
        self.reversed = false;
        self.moved(cx);
    }
    pub fn select_all(&mut self, _: &mut Window, cx: &mut Context<Self>) {
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
    fn changed(&mut self, cx: &mut Context<Self>) {
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
    fn apply(&mut self, range: Range<usize>, text: &str) {
        let mut value = String::with_capacity(self.content.len() - range.len() + text.len());
        value.push_str(&self.content[..range.start]);
        value.push_str(text);
        value.push_str(&self.content[range.end..]);
        self.content = value.into();
        self.selection = range.start + text.len()..range.start + text.len();
        self.reversed = false;
    }
    fn commit(&mut self, range: Range<usize>, text: &str, merge: bool, cx: &mut Context<Self>) {
        if self.content[range.clone()] == *text {
            self.selection = range.start + text.len()..range.start + text.len();
            self.reversed = false;
            self.marked = None;
            self.moved(cx);
            return;
        }
        let coalesce = merge
            && range.is_empty()
            && !text.contains(['\r', '\n'])
            && self
                .last_edit
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(700))
            && self
                .undo
                .last()
                .is_some_and(|e| e.before.is_empty() && e.start + e.after.len() == range.start);
        if coalesce {
            self.undo.last_mut().unwrap().after.push_str(text);
        } else {
            self.undo.push(Edit {
                start: range.start,
                before: self.content[range.clone()].into(),
                after: text.into(),
                selection: self.selection.clone(),
                reversed: self.reversed,
            });
        }
        self.last_edit = merge.then(std::time::Instant::now);
        self.redo.clear();
        self.apply(range, text);
        self.marked = None;
        self.changed(cx);
    }
    fn finish_composition(&mut self) {
        if let Some((before, selection, reversed)) = self.composition.take() {
            self.undo.push(Edit {
                start: 0,
                before: before.to_string(),
                after: self.content.to_string(),
                selection,
                reversed,
            });
            self.redo.clear();
        }
        self.marked = None;
    }
    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_composition();
        self.last_edit = None;
        if let Some(edit) = self.undo.pop() {
            let redo = Edit {
                selection: self.selection.clone(),
                reversed: self.reversed,
                ..edit.clone()
            };
            self.apply(edit.start..edit.start + edit.after.len(), &edit.before);
            self.selection = edit.selection;
            self.reversed = edit.reversed;
            self.redo.push(redo);
            self.changed(cx);
        }
    }
    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.last_edit = None;
        if let Some(edit) = self.redo.pop() {
            let undo = Edit {
                selection: self.selection.clone(),
                reversed: self.reversed,
                ..edit.clone()
            };
            self.apply(edit.start..edit.start + edit.before.len(), &edit.after);
            self.selection = edit.selection;
            self.reversed = edit.reversed;
            self.undo.push(undo);
            self.changed(cx);
        }
    }
    fn clipboard_range(&self) -> Range<usize> {
        if !self.selection.is_empty() || !self.multiline {
            return self.selection.clone();
        }
        // Logical line, including its original terminator (CRLF, CR or LF).
        let cursor = self.cursor();
        let start = self.content[..cursor]
            .rfind(['\r', '\n'])
            .map_or(0, |i| i + 1);
        let end = self.content[cursor..]
            .find(['\r', '\n'])
            .map_or(self.content.len(), |i| {
                let end = cursor + i;
                end + if self.content[end..].starts_with("\r\n") {
                    2
                } else {
                    1
                }
            });
        start..end
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let range = self.clipboard_range();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[range].into()));
        }
    }
    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        let range = self.clipboard_range();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[range.clone()].into(),
            ));
            self.finish_composition();
            self.commit(range, "", false, cx);
        }
    }
    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
            self.last_edit = None;
            self.replace(text, window, cx);
            self.last_edit = None;
        }
    }
    fn prev(&self, at: usize) -> usize {
        self.content[..at]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }
    fn next(&self, at: usize) -> usize {
        self.content[at..]
            .graphemes(true)
            .next()
            .map_or(at, |s| at + s.len())
    }
    fn word(&self, at: usize, backward: bool) -> usize {
        if backward {
            let prefix = &self.content[..at];
            let trim = prefix.trim_end_matches(char::is_whitespace);
            trim.unicode_word_indices()
                .next_back()
                .map_or(0, |(i, _)| i)
        } else {
            self.content[at..]
                .unicode_word_indices()
                .next()
                .map_or(self.content.len(), |(i, s)| at + i + s.len())
        }
    }
    fn move_to(&mut self, at: usize, shift: bool, cx: &mut Context<Self>) {
        self.last_edit = None;
        let at = self.boundary(at);
        let anchor = if !shift {
            at
        } else if self.reversed {
            self.selection.end
        } else {
            self.selection.start
        };
        self.selection = at.min(anchor)..at.max(anchor);
        self.reversed = at < anchor;
        self.moved(cx);
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let ctrl = event.keystroke.modifiers.control;
        let shift = event.keystroke.modifiers.shift;
        if event.keystroke.modifiers.alt || event.keystroke.modifiers.platform {
            return;
        }
        let cursor = self.cursor();
        let mut dest = None;
        match key {
            "left" => {
                dest = Some(if !shift && !self.selection.is_empty() {
                    self.selection.start
                } else if ctrl {
                    self.word(cursor, true)
                } else {
                    self.prev(cursor)
                })
            }
            "right" => {
                dest = Some(if !shift && !self.selection.is_empty() {
                    self.selection.end
                } else if ctrl {
                    self.word(cursor, false)
                } else {
                    self.next(cursor)
                })
            }
            "home" => {
                dest = Some(if ctrl {
                    0
                } else {
                    self.visual_edge(cursor, false)
                })
            }
            "end" => {
                dest = Some(if ctrl {
                    self.content.len()
                } else {
                    self.visual_edge(cursor, true)
                })
            }
            "up" | "down" | "pageup" | "pagedown" if self.multiline => {
                let p = self.position(cursor);
                let x = self.preferred_x.unwrap_or(p.x);
                self.preferred_x = Some(x);
                let step = if key.starts_with("page") {
                    self.bounds.size.height.max(self.line_height)
                } else {
                    self.line_height
                };
                dest = Some(self.index_at(point(
                    x,
                    p.y + if key == "up" || key == "pageup" {
                        -step
                    } else {
                        step
                    },
                )));
            }
            "backspace" | "delete" => {
                let range = if self.selection.is_empty() {
                    if key == "backspace" {
                        (if ctrl {
                            self.word(cursor, true)
                        } else {
                            self.prev(cursor)
                        })..cursor
                    } else {
                        cursor..if ctrl {
                            self.word(cursor, false)
                        } else {
                            self.next(cursor)
                        }
                    }
                } else {
                    self.selection.clone()
                };
                if !range.is_empty() {
                    self.commit(range, "", false, cx);
                }
            }
            "enter" => {
                if self.multiline {
                    self.replace("\n", window, cx);
                } else {
                    cx.emit(InputEvent::PressEnter { shift });
                }
            }
            "tab" if self.multiline && !ctrl => self.replace("\t", window, cx),
            _ => return,
        }
        if !matches!(key, "up" | "down" | "pageup" | "pagedown") {
            self.preferred_x = None;
        }
        if let Some(at) = dest {
            self.move_to(at, shift, cx);
        }
        cx.stop_propagation();
    }
    fn visual_edge(&self, at: usize, end: bool) -> usize {
        let p = self.position(at);
        self.index_at(point(if end { px(1e8) } else { px(0.) }, p.y))
    }
    fn position(&self, at: usize) -> Point<Pixels> {
        let i = self
            .lines
            .partition_point(|l| l.start <= at)
            .saturating_sub(1);
        self.lines.get(i).map_or(
            point(self.content_padding.left, self.content_padding.top),
            |p| {
                p.position(at.min(p.end) - p.start, self.line_height)
                    + point(self.content_padding.left, p.top)
            },
        )
    }
    fn index_at(&self, p: Point<Pixels>) -> usize {
        let p = point(p.x - self.content_padding.left, p.y);
        let i = self
            .lines
            .partition_point(|l| l.top <= p.y)
            .saturating_sub(1);
        self.lines.get(i).map_or(0, |l| {
            l.start
                + l.source_index(
                    l.line
                        .index_for_position(point(p.x, (p.y - l.top).max(px(0.))), self.line_height)
                        .unwrap_or_else(|i| i),
                )
        })
    }
    fn mouse_index(&self, position: Point<Pixels>) -> usize {
        self.index_at(position - self.bounds.origin - self.scroll)
    }
    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus(window, cx);
        if let Some((axis, track, thumb)) = [false, true]
            .into_iter()
            .filter_map(|axis| {
                self.scrollbar_geometry(axis)
                    .map(|(track, thumb)| (axis, track, thumb))
            })
            .find(|(_, track, _)| track.contains(&e.position))
        {
            let at = if axis { e.position.x } else { e.position.y };
            let viewport = if axis {
                track.size.width
            } else {
                track.size.height
            };
            let extent = if axis {
                self.content_size.width
            } else {
                self.content_size.height
            };
            let origin = if axis { track.left() } else { track.top() };
            let thumb_size = if axis {
                thumb.size.width
            } else {
                thumb.size.height
            };
            let thumb_start = if axis { thumb.left() } else { thumb.top() };
            let value = if at >= thumb_start && at <= thumb_start + thumb_size {
                if axis { self.scroll.x } else { self.scroll.y }
            } else {
                -((extent - viewport)
                    * ((at - origin - thumb_size / 2.) / (viewport - thumb_size).max(px(1.))))
                .clamp(px(0.), extent - viewport)
            };
            if axis {
                self.scroll.x = value;
            } else {
                self.scroll.y = value;
            }
            self.scroll_drag = Some((axis, at, value));
            self.reveal = false;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        self.selecting = true;
        let at = self.mouse_index(e.position);
        if e.click_count >= 3 {
            let start = self.content[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1);
            let end = self.content[at..]
                .find(['\n', '\r'])
                .map_or(self.content.len(), |i| at + i);
            self.set_selected_range(start..end, cx);
        } else if e.click_count == 2 {
            let range = self
                .content
                .unicode_word_indices()
                .find(|(i, s)| *i <= at && at < *i + s.len())
                .map(|(i, s)| i..i + s.len())
                .unwrap_or(at..self.next(at));
            self.set_selected_range(range, cx);
        } else {
            self.move_to(at, e.modifiers.shift, cx);
        }
        cx.stop_propagation();
    }
    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some((axis, start, offset)) = self.scroll_drag {
            let viewport = if axis {
                self.bounds.size.width
            } else {
                self.bounds.size.height
            };
            let extent = if axis {
                self.content_size.width
            } else {
                self.content_size.height
            };
            let at = if axis { e.position.x } else { e.position.y };
            let thumb = (viewport * (viewport / extent)).max(px(24.)).min(viewport);
            let value =
                offset - (extent - viewport) * ((at - start) / (viewport - thumb).max(px(1.)));
            if axis {
                self.scroll.x = value
            } else {
                self.scroll.y = value
            };
            self.clamp_scroll();
            self.reveal = false;
            cx.notify();
            return;
        }
        if self.selecting {
            if e.position.y < self.bounds.top() {
                self.scroll.y += self.line_height;
            }
            if e.position.y > self.bounds.bottom() {
                self.scroll.y -= self.line_height;
            }
            self.clamp_scroll();
            self.move_to(self.mouse_index(e.position), true, cx);
        }
    }
    fn clamp_scroll(&mut self) {
        self.scroll.x = self
            .scroll
            .x
            .min(px(0.))
            .max(-(self.content_size.width - self.bounds.size.width).max(px(0.)));
        self.scroll.y = self
            .scroll
            .y
            .min(px(0.))
            .max(-(self.content_size.height - self.bounds.size.height).max(px(0.)));
    }
    fn layout(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        self.bounds = bounds;
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        self.line_height = (font_size * 1.5).max(px(18.));
        if !self.multiline {
            let top = ((bounds.size.height - self.line_height) / 2.).max(px(0.));
            if self.content_padding.top != top {
                self.content_padding.top = top;
                self.layout_key = None;
            }
        }
        let width = if self.wrap {
            (bounds.size.width - self.content_padding.left - self.content_padding.right).max(px(1.))
        } else {
            px(1e8)
        };
        let display = if self.content.is_empty() {
            self.placeholder.clone()
        } else {
            self.content.clone()
        };
        let color = if self.content.is_empty() {
            style.color.opacity(0.5)
        } else {
            style.color
        };
        let key = (self.revision, width, font_size, style.font(), color);
        if self.layout_key.as_ref() != Some(&key) {
            self.lines.clear();
            let mut start = 0;
            let mut top = self.content_padding.top;
            let mut max_width = px(0.);
            loop {
                let end = display[start..]
                    .find(['\r', '\n'])
                    .map_or(display.len(), |i| start + i);
                let (value, tabs) = expand_tabs(&display[start..end]);
                let run = TextRun {
                    len: value.len(),
                    font: style.font(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                if let Ok(lines) = window.text_system().shape_text(
                    value,
                    font_size,
                    &[run],
                    self.wrap.then_some(width),
                    None,
                ) {
                    for line in lines {
                        max_width = max_width.max(line.width());
                        let height = line.size(self.line_height).height;
                        self.lines.push(Paragraph {
                            start,
                            end,
                            top,
                            line,
                            tabs: tabs.clone(),
                        });
                        top += height;
                    }
                }
                if end == display.len() {
                    break;
                }
                start = end
                    + if display[end..].starts_with("\r\n") {
                        2
                    } else {
                        1
                    };
            }
            self.content_size = size(
                if self.wrap {
                    bounds.size.width
                } else {
                    self.content_padding.left + max_width + self.content_padding.right.max(px(4.))
                },
                top.max(self.content_padding.top + self.line_height) + self.content_padding.bottom,
            );
            self.layout_key = Some(key);
        }
        if self.reveal {
            let caret = self.position(self.cursor());
            if self.cursor() == 0 {
                self.scroll = Point::default();
            }
            if caret.y + self.scroll.y < px(0.) {
                self.scroll.y = -(caret.y - self.content_padding.top).max(px(0.));
            }
            if caret.y + self.line_height + self.scroll.y > bounds.size.height {
                self.scroll.y = bounds.size.height - caret.y - self.line_height;
            }
            if caret.x + self.scroll.x < px(0.) {
                self.scroll.x = -(caret.x - self.content_padding.left).max(px(0.));
            }
            if caret.x + px(3.) + self.scroll.x > bounds.size.width {
                self.scroll.x = bounds.size.width - caret.x - px(3.);
            }
            self.reveal = false;
        }
        self.clamp_scroll();
    }
    #[cfg(test)]
    pub fn paint_clip(&self) -> Bounds<Pixels> {
        self.paint_clip
    }
    fn paint(&mut self, window: &mut Window, cx: &mut App) {
        #[cfg(test)]
        {
            self.paint_clip = window.content_mask().bounds;
        }
        let origin = self.bounds.origin + self.scroll;
        let focused = self.focus.is_focused(window);
        let selection_color = cx.theme().selection;
        let first = self
            .lines
            .partition_point(|l| l.top + l.line.size(self.line_height).height < -self.scroll.y);
        for l in &self.lines[first..] {
            if l.top + self.scroll.y > self.bounds.size.height {
                break;
            }
            let height = l.line.size(self.line_height).height;
            if l.top + height + self.scroll.y < px(0.)
                || l.top + self.scroll.y > self.bounds.size.height
            {
                continue;
            }
            let pos = origin + point(self.content_padding.left, l.top);
            let start = self.selection.start.max(l.start);
            let end = self.selection.end.min(l.end);
            if start <= end && self.selection.end > l.start && self.selection.start <= l.end {
                let a = l.position(start - l.start, self.line_height);
                let b = l.position(end - l.start, self.line_height);
                let mut y = a.y;
                while y <= b.y {
                    let x = if y == a.y { a.x } else { px(0.) };
                    let right = if y == b.y && self.selection.end <= l.end {
                        b.x
                    } else {
                        self.bounds.size.width - self.scroll.x - self.content_padding.left
                    };
                    window.paint_quad(fill(
                        Bounds::new(
                            pos + point(x, y),
                            size((right - x).max(px(2.)), self.line_height),
                        ),
                        selection_color,
                    ));
                    y += self.line_height;
                }
            }
            let _ = l
                .line
                .paint(pos, self.line_height, TextAlign::Left, None, window, cx);
            if let Some(marked) = &self.marked {
                let start = marked.start.max(l.start);
                let end = marked.end.min(l.end);
                if start < end {
                    let a = l.position(start - l.start, self.line_height);
                    let b = l.position(end - l.start, self.line_height);
                    let mut y = a.y;
                    while y <= b.y {
                        let x = if y == a.y { a.x } else { px(0.) };
                        let right = if y == b.y {
                            b.x
                        } else {
                            self.bounds.size.width - self.scroll.x - self.content_padding.left
                        };
                        window.paint_quad(fill(
                            Bounds::new(
                                pos + point(x, y + self.line_height - px(2.)),
                                size((right - x).max(px(1.)), px(1.)),
                            ),
                            cx.theme().foreground,
                        ));
                        y += self.line_height;
                    }
                }
            }
        }
        if focused && self.blink {
            window.paint_quad(fill(
                Bounds::new(
                    origin + self.position(self.cursor()),
                    size(px(1.5), self.line_height),
                ),
                cx.theme().foreground,
            ));
        }
        for axis in [false, true] {
            if let Some((track, thumb)) = self.scrollbar_geometry(axis) {
                let active = self
                    .scroll_drag
                    .is_some_and(|(drag_axis, _, _)| drag_axis == axis);
                let hovered = self.scroll_hover == Some(axis);
                if active || hovered {
                    window.paint_quad(fill(track, cx.theme().primary.opacity(0.1)));
                }
                window.paint_quad(fill(
                    thumb,
                    if active || hovered {
                        cx.theme().primary
                    } else {
                        cx.theme().muted_foreground.opacity(0.5)
                    },
                ));
            }
        }
    }
    fn scrollbar_geometry(&self, horizontal: bool) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
        if !self.multiline || (horizontal && self.wrap) {
            return None;
        }
        let viewport = if horizontal {
            self.bounds.size.width
        } else {
            self.bounds.size.height
        };
        let content = if horizontal {
            self.content_size.width
        } else {
            self.content_size.height
        };
        if content <= viewport || viewport <= px(0.) {
            return None;
        }
        let length = (viewport * (viewport / content)).max(px(24.)).min(viewport);
        let offset = if horizontal {
            self.scroll.x
        } else {
            self.scroll.y
        };
        let pos = (viewport - length) * (-offset / (content - viewport));
        Some(if horizontal {
            (
                Bounds::new(
                    point(self.bounds.left(), self.bounds.bottom() - px(10.)),
                    size(viewport, px(10.)),
                ),
                Bounds::new(
                    point(self.bounds.left() + pos, self.bounds.bottom() - px(8.)),
                    size(length, px(6.)),
                ),
            )
        } else {
            (
                Bounds::new(
                    point(self.bounds.right() - px(10.), self.bounds.top()),
                    size(px(10.), viewport),
                ),
                Bounds::new(
                    point(self.bounds.right() - px(8.), self.bounds.top() + pos),
                    size(px(6.), length),
                ),
            )
        })
    }
    fn utf8(&self, offset: usize) -> usize {
        utf8_offset(&self.content, offset)
    }
    fn utf16(&self, offset: usize) -> usize {
        self.content[..self.boundary(offset)].encode_utf16().count()
    }
    fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.utf8(r.start)..self.utf8(r.end)
    }
    fn to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.utf16(r.start)..self.utf16(r.end)
    }
}
fn utf8_offset(text: &str, n: usize) -> usize {
    let mut units = 0;
    for (i, ch) in text.char_indices() {
        if units >= n {
            return i;
        }
        units += ch.len_utf16();
    }
    text.len()
}
impl EntityInputHandler for InputState {
    fn text_for_range(
        &mut self,
        r: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = self.range_from_utf16(&r);
        *actual = Some(self.to_utf16(&r));
        Some(self.content[r].into())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.to_utf16(&self.selection),
            reversed: self.reversed,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|r| self.to_utf16(r))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_composition();
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let r = r
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.clone());
        let text = if self.multiline {
            text.to_string()
        } else {
            text.replace(['\r', '\n'], " ")
        };
        if self.composition.is_some() {
            let changed = self.content[r.clone()] != *text;
            self.apply(r, &text);
            self.finish_composition();
            if changed {
                self.changed(cx);
            } else {
                self.moved(cx);
            }
        } else {
            self.commit(r, &text, true, cx);
        }
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composition.is_none() {
            self.composition = Some((self.content.clone(), self.selection.clone(), self.reversed));
        }
        let r = r
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.clone());
        let start = r.start;
        self.apply(r, text);
        self.marked = Some(start..start + text.len());
        if let Some(r) = selection {
            self.selection = start + utf8_offset(text, r.start)..start + utf8_offset(text, r.end);
        }
        self.changed(cx);
    }
    fn bounds_for_range(
        &mut self,
        r: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let r = self.range_from_utf16(&r);
        let a = self.position(r.start);
        let b = self.position(r.end);
        Some(Bounds::new(
            self.bounds.origin + self.scroll + a,
            size(
                if a.y == b.y {
                    (b.x - a.x).max(px(1.))
                } else {
                    px(1.)
                },
                self.line_height,
            ),
        ))
    }
    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.utf16(self.mouse_index(p)))
    }
}

struct EditorElement(Entity<InputState>);
impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = (Vec<(bool, Hitbox)>, Option<Hitbox>);
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.0.update(cx, |s, _| s.layout(bounds, window));
        let state = self.0.read(cx);
        let capture = state.scroll_drag.map(|_| {
            window.insert_hitbox(
                Bounds::new(point(px(0.), px(0.)), window.viewport_size()),
                HitboxBehavior::BlockMouse,
            )
        });
        let bars = [true, false]
            .into_iter()
            .filter_map(|axis| {
                state.scrollbar_geometry(axis).map(|(track, _)| {
                    (
                        axis,
                        window.insert_hitbox(track, HitboxBehavior::BlockMouseExceptScroll),
                    )
                })
            })
            .collect();
        (bars, capture)
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        bars: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let hovered = bars
            .0
            .iter()
            .find(|(_, hitbox)| hitbox.is_hovered(window))
            .map(|(axis, _)| *axis);
        self.0.update(cx, |s, _| s.scroll_hover = hovered);
        for (_, hitbox) in &bars.0 {
            window.set_cursor_style(CursorStyle::Arrow, hitbox);
        }
        if let Some(hitbox) = &bars.1 {
            window.set_cursor_style(CursorStyle::Arrow, hitbox);
        }
        let entity = self.0.clone();
        let hitboxes = bars.0.clone();
        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && e.button == MouseButton::Left
                && hitboxes.iter().any(|(_, h)| h.is_hovered(window))
            {
                entity.update(cx, |s, cx| s.mouse_down(e, window, cx));
            }
        });
        let entity = self.0.clone();
        let hitboxes = bars.0.clone();
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }
            entity.update(cx, |s, cx| {
                let hover = hitboxes
                    .iter()
                    .find(|(_, h)| h.is_hovered(window))
                    .map(|(axis, _)| *axis);
                if s.scroll_hover != hover {
                    s.scroll_hover = hover;
                    cx.notify();
                }
                if s.scroll_drag.is_some() {
                    s.mouse_move(e, window, cx);
                    cx.stop_propagation();
                }
            });
        });
        let entity = self.0.clone();
        window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && e.button == MouseButton::Left {
                entity.update(cx, |s, cx| {
                    if s.scroll_drag.take().is_some() {
                        cx.notify();
                        cx.stop_propagation();
                    }
                });
            }
        });
        let focus = self.0.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.0.clone()), cx);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.0.update(cx, |s, cx| s.paint(window, cx))
        });
    }
}
impl Render for InputState {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("text-input")
            .debug_selector(|| "text-input".into())
            .role(if self.multiline {
                Role::MultilineTextInput
            } else {
                Role::TextInput
            })
            .aria_label(self.label.clone())
            .aria_value(self.content.clone())
            .aria_placeholder(self.placeholder.clone())
            .size_full()
            .overflow_hidden()
            .key_context("EdotInput")
            .track_focus(&self.focus)
            .tab_index(0)
            .tab_stop(true)
            .cursor_text()
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(|s, _: &SelectAll, w, cx| s.select_all(w, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|s, _, _, _| {
                    s.selecting = false;
                    s.scroll_drag = None;
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|s, _, _, _| {
                    s.selecting = false;
                    s.scroll_drag = None;
                }),
            )
            .on_scroll_wheel(cx.listener(|s, e: &ScrollWheelEvent, _, cx| {
                if !e.modifiers.control {
                    let mut delta = e.delta.pixel_delta(s.line_height);
                    if e.modifiers.shift {
                        // Windows already maps Shift+wheel to x. Only convert
                        // platforms/events that still provide a vertical delta.
                        if delta.x == px(0.) {
                            delta.x = delta.y;
                        }
                        delta.y = px(0.);
                        s.wheel_gesture = OngoingScroll::default();
                    } else if e.delta.precise() {
                        // Preserve the left edge during a mostly vertical
                        // trackpad gesture instead of accumulating sideways noise.
                        s.wheel_gesture.filter(&mut delta, e.touch_phase);
                    }
                    s.scroll += delta;
                    s.clamp_scroll();
                    s.reveal = false;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .child(EditorElement(cx.entity()))
    }
}

#[derive(IntoElement)]
pub struct Input {
    state: Entity<InputState>,
    style: StyleRefinement,
    appearance: bool,
    label: SharedString,
    suffix: Vec<AnyElement>,
}
pub type Textarea = Input;
impl Input {
    pub fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            appearance: true,
            label: "".into(),
            suffix: vec![],
        }
    }
    pub fn suffix(mut self, child: impl IntoElement) -> Self {
        self.suffix.push(child.into_any_element());
        self
    }
    pub fn small(self) -> Self {
        self.h(px(32.))
    }
    pub fn appearance(mut self, value: bool) -> Self {
        self.appearance = value;
        self
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }
}
impl Styled for Input {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl RenderOnce for Input {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state
            .update(cx, |state, _| state.label = self.label.clone());
        let focus = self.state.read(cx).focus.clone();
        let label = self.label.clone();
        let mut el = div()
            .id(("input", self.state.entity_id()))
            .debug_selector(move || label.to_string())
            .track_focus(&focus)
            .h_flex()
            .w_full()
            .h(px(34.))
            .min_w_0()
            .px_2()
            .py_0()
            .aria_label(self.label)
            .when(self.appearance, |d| {
                d.border_1()
                    .rounded_md()
                    .border_color(cx.theme().border)
                    .focus(|s| s.border_color(cx.theme().primary))
                    .bg(cx.theme().background)
            });
        el.style().refine(&self.style);
        el.child(div().flex_1().min_w_0().h_full().child(self.state))
            .children(self.suffix)
    }
}
