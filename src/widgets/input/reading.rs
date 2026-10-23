//! A cached, virtualized GPUI reading view; source editing state stays untouched.
use super::super::scroll::AutoScroll;
use super::{markdown::*, reading_document::*, *};
use std::path::PathBuf;
use std::sync::Arc;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
struct Fragment {
    layout: TextLayout,
    offset: usize,
    tabs: Vec<(usize, usize)>,
    /// Horizontal painting offset for each visual row of aligned text.
    aligned_rows: Vec<Pixels>,
}
impl Fragment {
    fn index(&self, mut position: Point<Pixels>) -> (usize, bool) {
        let row = ((position.y - self.layout.bounds().top()).max(px(0.))
            / self.layout.line_height())
        .floor() as usize;
        if let Some(offset) = self.aligned_rows.get(row) {
            // GPUI paints text alignment but its TextLayout hit test uses the
            // unaligned origin. Match each visual row's painted glyph position.
            position.x -= *offset;
        }
        let hit = self.layout.index_for_position(position);
        let index = *hit.as_ref().unwrap_or_else(|i| i);
        let mut expansion = 0;
        for &(source, display) in &self.tabs {
            if index < display {
                break;
            }
            if index < display + 4 {
                return (
                    self.offset + source + usize::from(index - display >= 2),
                    hit.is_ok(),
                );
            }
            expansion += 3;
        }
        (self.offset + index - expansion, hit.is_ok())
    }
}
#[derive(Clone)]
struct ReadingSelection {
    layouts: Rc<RefCell<Vec<Fragment>>>,
    range: Range<usize>,
}
impl ReadingSelection {
    fn hit(&self, position: Point<Pixels>) -> Option<(usize, bool)> {
        self.layouts
            .borrow()
            .iter()
            .min_by(|a, b| {
                let distance = |f: &Fragment| {
                    let bounds = f.layout.bounds();
                    let x = f32::from(
                        (bounds.left() - position.x)
                            .max(position.x - bounds.right())
                            .max(px(0.)),
                    );
                    let y = f32::from(
                        (bounds.top() - position.y)
                            .max(position.y - bounds.bottom())
                            .max(px(0.)),
                    );
                    // Prefer the closest line, then the closest cell in that line.
                    y * 10000. + x
                };
                distance(a).total_cmp(&distance(b))
            })
            .map(|f| f.index(position))
    }
}

struct SelectableText {
    text: StyledText,
    fragment: Fragment,
    layouts: Rc<RefCell<Vec<Fragment>>>,
}
impl IntoElement for SelectableText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for SelectableText {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        w: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        self.text.request_layout(id, inspector, w, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        w: &mut Window,
        cx: &mut App,
    ) {
        self.text.prepaint(id, inspector, bounds, state, w, cx);
        if self.fragment.layout.len() == 0 {
            return;
        }
        self.fragment.aligned_rows.clear();
        let factor = match w.text_style().text_align {
            TextAlign::Left => 0.,
            TextAlign::Center => 0.5,
            TextAlign::Right => 1.,
        };
        if factor != 0. {
            for line in self.fragment.layout.line_layouts() {
                let mut start = px(0.);
                for end in line
                    .wrap_boundaries
                    .iter()
                    .map(|b| {
                        line.unwrapped_layout.runs[b.run_ix].glyphs[b.glyph_ix]
                            .position
                            .x
                    })
                    .chain(std::iter::once(line.unwrapped_layout.width))
                {
                    self.fragment
                        .aligned_rows
                        .push((bounds.size.width - (end - start)) * factor);
                    start = end;
                }
            }
        }
        let mut layouts = self.layouts.borrow_mut();
        layouts.retain(|f| f.offset != self.fragment.offset);
        layouts.push(self.fragment.clone());
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        prepaint: &mut (),
        w: &mut Window,
        cx: &mut App,
    ) {
        self.text
            .paint(id, inspector, bounds, state, prepaint, w, cx);
    }
}

pub(super) struct Reading {
    pub(super) document: Arc<Document>,
    revision: Option<u64>,
    task: Option<Task<()>>,
    pub(super) list: ListState,
    font: Option<(Font, Pixels)>,
    directory: PathBuf,
    anchor: usize,
    head: usize,
    drag: Option<Point<Pixels>>,
    dragged: bool,
    link: Option<String>,
    copied_block: Option<usize>,
    copy_feedback: Option<Task<()>>,
    layouts: Rc<RefCell<Vec<Fragment>>>,
    auto_scroll: AutoScroll,
}
impl Default for Reading {
    fn default() -> Self {
        Self {
            document: Arc::new(Document::default()),
            revision: None,
            task: None,
            list: ListState::new(0, ListAlignment::Top, px(200.)),
            font: None,
            directory: PathBuf::new(),
            anchor: 0,
            head: 0,
            drag: None,
            dragged: false,
            link: None,
            copied_block: None,
            copy_feedback: None,
            layouts: Rc::default(),
            auto_scroll: AutoScroll::default(),
        }
    }
}
impl InputState {
    pub(super) fn is_reading_dragging(&self) -> bool {
        self.reading.drag.is_some()
    }
    pub(super) fn stop_reading_drag(&mut self) {
        self.reading.drag = None;
        self.reading.link = None;
        self.reading.auto_scroll.stop();
    }
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }
    pub(super) fn is_markdown_reading(&self) -> bool {
        self.read_only && self.text_mode == TextMode::Markdown
    }
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        if self.read_only == read_only {
            return;
        }
        if read_only {
            self.finish_composition();
        }
        self.read_only = read_only;
        self.blink = true;
        self.stop_pointer_drag();
        self.layout_key = None;
        self.ensure_highlighting(cx);
        self.ensure_reading(cx);
        cx.notify();
    }
    pub fn source_directory(mut self, directory: PathBuf) -> Self {
        self.reading.directory = directory;
        self
    }
    fn install_reading(&mut self, document: Document, revision: u64, cx: &mut Context<Self>) {
        let offset = self.reading.list.logical_scroll_top();
        self.reading.anchor = 0;
        self.reading.head = 0;
        self.reading.drag = None;
        self.reading.link = None;
        self.reading.auto_scroll.stop();
        self.reading.layouts.borrow_mut().clear();
        self.reading.copied_block = None;
        self.reading.copy_feedback = None;
        self.reading.document = Arc::new(document);
        self.reading.revision = Some(revision);
        self.reading.list.reset(self.reading.document.blocks.len());
        if self.reveal {
            self.reveal_reading();
        } else {
            self.reading.list.scroll_to(offset);
        }
        cx.notify();
    }
    fn reading_selection(&self) -> ReadingSelection {
        ReadingSelection {
            layouts: self.reading.layouts.clone(),
            range: self.reading.anchor.min(self.reading.head)
                ..self.reading.anchor.max(self.reading.head),
        }
    }
    fn copy_reading_block(&mut self, index: usize, revision: u64, cx: &mut Context<Self>) {
        if !self.is_markdown_reading()
            || revision != self.revision
            || self.reading.revision != Some(revision)
        {
            return;
        }
        let Some(block) = self.reading.document.blocks.get(index) else {
            return;
        };
        if !matches!(block.kind, Kind::Code | Kind::Mermaid(_)) {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(block.content.text.clone()));
        self.reading.copied_block = Some(index);
        self.reading.copy_feedback = Some(cx.spawn(async |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |s, cx| {
                s.reading.copied_block = None;
                s.reading.copy_feedback = None;
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn toggle_reading_task(
        &mut self,
        index: usize,
        revision: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_markdown_reading()
            || self.is_composing()
            || revision != self.revision
            || self.reading.revision != Some(revision)
        {
            return false;
        }
        let Some(block) = self.reading.document.blocks.get(index) else {
            return false;
        };
        let Some(marker) = block.task_marker else {
            return false;
        };
        let replacement = match (
            block.prefix.as_str(),
            self.content.get(marker - 1..marker + 2),
        ) {
            ("☐", Some("[ ]")) => "x",
            ("☑", Some("[x]" | "[X]")) => " ",
            _ => return false,
        };
        let selection = (
            self.selection.clone(),
            self.reversed,
            self.caret_after_wrap,
            self.extra.clone(),
            self.occurrence_word,
        );
        let offset = self.reading.list.logical_scroll_top();
        self.commit(marker..marker + 1, replacement, false, cx);
        self.undo.last_mut().unwrap().task_toggle = true;
        (
            self.selection,
            self.reversed,
            self.caret_after_wrap,
            self.extra,
            self.occurrence_word,
        ) = selection;
        self.reveal = false;
        self.reading.list.scroll_to(offset);
        self.ensure_reading(cx);
        self.focus(window, cx);
        true
    }
    fn reading_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(start) = self.reading.drag else {
            return;
        };
        if !self.is_markdown_reading() {
            self.reading.drag = None;
            self.reading.auto_scroll.stop();
            return;
        }
        let previous = (self.reading.dragged, self.reading.head);
        self.reading.dragged |=
            (position.x - start.x).abs() > px(3.) || (position.y - start.y).abs() > px(3.);
        if self.reading.dragged {
            if let Some((index, _)) = self.reading_selection().hit(position) {
                self.reading.head = index.min(self.reading.document.len);
            }
            if previous != (self.reading.dragged, self.reading.head) {
                cx.notify();
            }
        }
    }
    pub(super) fn reveal_reading(&self) {
        if self.is_markdown_reading() {
            let index = self
                .reading
                .document
                .blocks
                .partition_point(|b| b.source.end <= self.cursor());
            if index < self.reading.document.blocks.len() {
                self.reading.list.scroll_to_reveal_item(index);
            }
        }
    }
    pub(super) fn ensure_reading(&mut self, cx: &mut Context<Self>) {
        if !self.is_markdown_reading() || self.reading.revision == Some(self.revision) {
            return;
        }
        if self.content.len() <= 64 * 1024 {
            self.install_reading(
                super::reading_document::parse(&self.content),
                self.revision,
                cx,
            );
            return;
        }
        if self.reading.task.is_some() {
            return;
        }
        let mut snapshot = (self.revision, self.content.clone());
        self.reading.task = Some(cx.spawn(async move |this, cx| {
            loop {
                let (revision, content) = snapshot;
                let document = cx
                    .background_executor()
                    .spawn(async move { super::reading_document::parse(&content) })
                    .await;
                let Ok(next) = this.update(cx, |s, cx| {
                    if s.is_markdown_reading() && s.revision == revision {
                        s.install_reading(document, revision, cx);
                    } else if s.is_markdown_reading() && s.reading.revision != Some(s.revision) {
                        return Some((s.revision, s.content.clone()));
                    }
                    s.reading.task = None;
                    None
                }) else {
                    return;
                };
                let Some(next) = next else {
                    return;
                };
                snapshot = next;
            }
        }));
    }
    pub(super) fn reading_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_reading(cx);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let font = (style.font(), font_size);
        if self.reading.font.as_ref() != Some(&font) {
            let offset = self.reading.list.logical_scroll_top();
            self.reading.list.reset(self.reading.document.blocks.len());
            self.reading.list.scroll_to(offset);
            self.reading.font = Some(font);
        }
        let document = self.reading.document.clone();
        let directory = self.reading.directory.clone();
        self.reading.layouts.borrow_mut().clear();
        let selection = self.reading_selection();
        let move_entity = cx.entity();
        let up_entity = cx.entity();
        let copy_entity = cx.entity().downgrade();
        let revision = self.revision;
        let copied_block = self.reading.copied_block;
        let reading = div()
            .id("markdown-reading")
            .debug_selector(|| "markdown-reading".into())
            .relative()
            .size_full()
            .overflow_hidden()
            .cursor(CursorStyle::IBeam)
            .track_focus(&self.focus)
            .tab_index(0)
            .tab_stop(true)
            .key_context("EdotInput")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, e: &MouseDownEvent, w, cx| {
                    s.focus(w, cx);
                    if let Some((index, exact)) = s.reading_selection().hit(e.position) {
                        if !e.modifiers.shift {
                            s.reading.anchor = index;
                        }
                        s.reading.head = index;
                        s.reading.drag = Some(e.position);
                        s.reading.dragged = e.modifiers.shift;
                        s.reading.link = None;
                        if exact && !e.modifiers.shift {
                            let block = s
                                .reading
                                .document
                                .blocks
                                .partition_point(|b| b.offset <= index)
                                .saturating_sub(1);
                            if let Some(block) = s.reading.document.blocks.get(block) {
                                s.reading.link = std::iter::once(&block.content)
                                    .chain(&block.cells)
                                    .find_map(|inline| {
                                        inline
                                            .links
                                            .iter()
                                            .find(|(range, _)| {
                                                (inline.offset + range.start
                                                    ..inline.offset + range.end)
                                                    .contains(&index)
                                            })
                                            .map(|(_, target)| target.clone())
                                    });
                            }
                        }
                    }
                    cx.notify();
                }),
            )
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(|s, _: &Copy, _, cx| {
                if s.reading.revision != Some(s.revision) {
                    return;
                }
                let text = s.reading.document.text_range(s.reading_selection().range);
                if !text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }))
            .on_action(cx.listener(|s, _: &SelectAll, _, cx| {
                if s.reading.revision != Some(s.revision) {
                    return;
                }
                s.reading.anchor = 0;
                s.reading.head = s.reading.document.len;
                s.reading.link = None;
                cx.notify();
            }))
            .on_key_down(cx.listener(|s, e: &KeyDownEvent, w, cx| {
                if e.prefer_character_input {
                    return;
                }
                let modifiers = e.keystroke.modifiers;
                let mut key = e.keystroke.key.as_str();
                if cfg!(target_os = "macos")
                    && (modifiers.platform || modifiers.control || modifiers.alt)
                {
                    key = match key {
                        "up" if super::super::shortcuts::primary(modifiers) => "home",
                        "down" if super::super::shortcuts::primary(modifiers) => "end",
                        _ => return,
                    };
                }
                match key {
                    "up" => s.reading.list.scroll_by(-px(36.)),
                    "down" => s.reading.list.scroll_by(px(36.)),
                    "pageup" => s.reading.list.scroll_by(-w.viewport_size().height * 0.8),
                    "pagedown" | "space" => {
                        s.reading.list.scroll_by(w.viewport_size().height * 0.8)
                    }
                    "home" => s.reading.list.scroll_to(ListOffset::default()),
                    "end" => s.reading.list.scroll_to_end(),
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }));
        if self.reading.revision != Some(self.revision) {
            return reading
                .child(
                    div()
                        .p_4()
                        .text_color(cx.theme().muted_foreground)
                        .child("읽기 화면 준비 중…"),
                )
                .into_any_element();
        }
        reading
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, cx| {
                        move_entity.update(cx, |s, cx| {
                            if let Some(position) = s.reading.auto_scroll.last_drag_position {
                                s.reading_drag(position, cx);
                            }
                        });
                        let entity = move_entity.clone();
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            entity.update(cx, |s, cx| {
                                if s.reading.drag.is_none() {
                                    return;
                                }
                                if e.pressed_button != Some(MouseButton::Left) {
                                    s.stop_reading_drag();
                                    cx.notify();
                                    cx.stop_propagation();
                                    return;
                                }
                                s.reading_drag(e.position, cx);
                                s.reading.auto_scroll.last_drag_position = Some(e.position);
                                let delta = s
                                    .reading
                                    .dragged
                                    .then(|| {
                                        AutoScroll::compute_delta(
                                            e.position.y,
                                            s.reading.list.viewport_bounds(),
                                        )
                                    })
                                    .flatten();
                                s.reading.auto_scroll.set(delta, cx, |delta, s, cx| {
                                    if !s.is_markdown_reading() || s.reading.drag.is_none() {
                                        s.reading.auto_scroll.stop();
                                        return;
                                    }
                                    s.reading.list.scroll_by(delta);
                                    cx.notify();
                                });
                                cx.stop_propagation();
                            });
                        });
                        let entity = up_entity.clone();
                        window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture || e.button != MouseButton::Left {
                                return;
                            }
                            entity.update(cx, |s, cx| {
                                if s.reading.drag.is_none() {
                                    return;
                                }
                                s.reading_drag(e.position, cx);
                                s.reading.drag = None;
                                s.reading.auto_scroll.stop();
                                if let Some(target) = s.reading.link.take()
                                    && !s.reading.dragged
                                    && s.reading.anchor == s.reading.head
                                    && (web_url(&target)
                                        || target
                                            .get(..7)
                                            .is_some_and(|s| s.eq_ignore_ascii_case("mailto:")))
                                {
                                    cx.open_url(&target);
                                }
                                cx.notify();
                                cx.stop_propagation();
                            });
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .children(self.pointer_drag_capture())
            .child(
                list(self.reading.list.clone(), move |i, window, cx| {
                    let block = &document.blocks[i];
                    let view = ReadingView {
                        window,
                        editor: copy_entity.clone(),
                        revision,
                    };
                    let rendered =
                        render_block(block, i, font_size, &directory, &selection, &view, cx);
                    if !matches!(block.kind, Kind::Code | Kind::Mermaid(_)) {
                        return rendered;
                    }
                    let entity = copy_entity.clone();
                    let copied = copied_block == Some(i);
                    div()
                        .id(("reading-code-container", i))
                        .relative()
                        .group("reading-code")
                        .child(rendered)
                        .child(
                            div()
                                .debug_selector(move || format!("reading-code-copy-{i}"))
                                .absolute()
                                .top(px(14.))
                                .right(px(22.))
                                .opacity(if copied { 1. } else { 0. })
                                .group_hover("reading-code", |s| s.opacity(1.))
                                .child(
                                    Button::new(("reading-code-copy", i))
                                        .small()
                                        .tab_stop(false)
                                        .icon(if copied {
                                            IconName::Check
                                        } else {
                                            IconName::Copy
                                        })
                                        .tooltip(if copied { "복사됨" } else { "코드 복사" })
                                        .bg(cx.theme().background)
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .on_click(move |_, _, cx| {
                                            let _ = entity.update(cx, |s, cx| {
                                                s.copy_reading_block(i, revision, cx);
                                            });
                                        }),
                                ),
                        )
                        .into_any_element()
                })
                .size_full(),
            )
            .child(super::super::scroll::Scrollbar::vertical_list(
                &self.reading.list,
            ))
            .into_any_element()
    }
}

fn render_inline(inline: &Inline, selection: &ReadingSelection, cx: &App) -> AnyElement {
    if inline.text.is_empty() && !inline.images.is_empty() {
        return div().into_any_element();
    }
    render_text(
        &inline.text,
        &inline.spans,
        inline.offset,
        false,
        selection,
        cx,
    )
}

fn render_text(
    text: &str,
    spans: &[Span],
    offset: usize,
    expand_tabs: bool,
    selection: &ReadingSelection,
    cx: &App,
) -> AnyElement {
    let mut tabs = Vec::new();
    if expand_tabs {
        for (index, _) in text.match_indices('\t') {
            tabs.push((index, index + tabs.len() * 3));
        }
    }
    let display_index = |index| index + tabs.partition_point(|(source, _)| *source < index) * 3;
    let selected = selection.range.start.saturating_sub(offset).min(text.len())
        ..selection.range.end.saturating_sub(offset).min(text.len());
    let mut boundaries = vec![0, text.len(), selected.start, selected.end];
    for span in spans {
        boundaries.extend([span.range.start, span.range.end]);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let primary = cx.theme().primary;
    let mut span_index = 0;
    let highlights: Vec<_> = boundaries
        .windows(2)
        .map(|ends| {
            let range = ends[0]..ends[1];
            while spans
                .get(span_index)
                .is_some_and(|s| s.range.end <= range.start)
            {
                span_index += 1;
            }
            let style = spans
                .get(span_index)
                .filter(|s| s.range.contains(&range.start))
                .map_or(0, |s| s.style);
            let mut h = HighlightStyle::default();
            if style & STRONG != 0 {
                h.font_weight = Some(FontWeight::BOLD);
            }
            if style & EMPHASIS != 0 {
                h.font_style = Some(FontStyle::Italic);
            }
            if style & STRIKE != 0 {
                h.strikethrough = Some(StrikethroughStyle {
                    thickness: px(1.),
                    color: None,
                });
            }
            if style & CODE != 0 {
                h.background_color = Some(cx.theme().accent);
            }
            if style & LINK != 0 {
                h.color = Some(primary);
                h.underline = Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(primary),
                    wavy: false,
                });
            }
            if selected.contains(&range.start) {
                h.background_color = Some(cx.theme().selection);
            }
            (display_index(range.start)..display_index(range.end), h)
        })
        .collect();
    let display = if expand_tabs {
        text.replace('\t', "    ")
    } else {
        text.to_owned()
    };
    let text = StyledText::new(display).with_highlights(highlights);
    let fragment = Fragment {
        layout: text.layout().clone(),
        offset,
        tabs,
        aligned_rows: Vec::new(),
    };
    SelectableText {
        text,
        fragment,
        layouts: selection.layouts.clone(),
    }
    .into_any_element()
}

fn render_image(image: &ReadingImage, directory: &std::path::Path) -> AnyElement {
    let source = &image.source;
    let alt = image.alt.clone();
    let fallback = move || {
        div()
            .debug_selector(|| "reading-image-fallback".into())
            .v_flex()
            .gap_1()
            .when(!alt.is_empty(), |d| {
                d.child(
                    div()
                        .debug_selector(|| "reading-image-alt".into())
                        .child(alt.clone()),
                )
            })
            .child("이미지를 불러올 수 없습니다")
            .into_any_element()
    };
    let image: ImageSource = if web_url(source) {
        source.clone().into()
    } else if source.contains("://") || source.starts_with("data:") {
        return fallback();
    } else {
        directory.join(source).into()
    };
    img(image)
        .w_full()
        .h(px(240.))
        .object_fit(ObjectFit::Contain)
        .with_fallback(fallback)
        .into_any_element()
}

struct ReadingView<'a> {
    window: &'a Window,
    editor: WeakEntity<InputState>,
    revision: u64,
}

fn render_task_marker(
    block: &Block,
    checked: bool,
    index: usize,
    font_size: Pixels,
    selection: &ReadingSelection,
    view: &ReadingView<'_>,
    cx: &App,
) -> AnyElement {
    let size = (font_size * 1.1).max(px(12.));
    let height = view.window.line_height().max(size);
    let selected = selection.range.start < block.offset + block.prefix.len()
        && selection.range.end > block.offset;
    let editor = view.editor.clone();
    let revision = view.revision;
    div()
        .id(("reading-task", index))
        .debug_selector(move || format!("reading-task-{index}"))
        .role(Role::CheckBox)
        .aria_label(if checked {
            "완료된 항목"
        } else {
            "미완료 항목"
        })
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .relative()
        .block_mouse_except_scroll()
        .cursor_pointer()
        .flex_shrink_0()
        .w(size)
        .h(height)
        .rounded(px(3.))
        .when(selected, |d| d.bg(cx.theme().selection))
        .hover(|d| d.bg(cx.theme().primary.opacity(0.1)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            let _ = editor.update(cx, |s, cx| {
                s.toggle_reading_task(index, revision, window, cx);
            });
            cx.stop_propagation();
        })
        // Retain the prefix's text layout for hit testing and rendered-text copy.
        .child(div().absolute().inset_0().opacity(0.).child(render_text(
            &block.prefix,
            &[],
            block.offset,
            false,
            selection,
            cx,
        )))
        .child(
            div()
                .absolute()
                .left_0()
                .top((height - size) / 2.)
                .size(size)
                .flex()
                .items_center()
                .justify_center()
                .rounded(size * 0.25)
                .border((font_size / 14.).max(px(1.)))
                .border_color(if checked {
                    cx.theme().primary
                } else {
                    cx.theme().muted_foreground.opacity(0.65)
                })
                .bg(if checked {
                    cx.theme().primary
                } else {
                    cx.theme().background
                })
                .when(checked, |d| {
                    d.child(
                        Icon::new(IconName::Check)
                            .size(size * 0.8)
                            .text_color(cx.theme().primary_foreground),
                    )
                }),
        )
        .into_any_element()
}

fn render_block(
    block: &Block,
    index: usize,
    font_size: Pixels,
    directory: &std::path::Path,
    selection: &ReadingSelection,
    view: &ReadingView<'_>,
    cx: &App,
) -> AnyElement {
    let window = view.window;
    let task = match block.prefix.as_str() {
        "☐" => Some(false),
        "☑" => Some(true),
        _ => None,
    };
    let mut body = div().v_flex().min_w_0().flex_1();
    match &block.kind {
        Kind::Rule => {
            body = body.child(div().h(px(1.)).my_2().bg(cx.theme().border));
        }
        Kind::Heading(level) => {
            let scale = match level {
                1 => 1.8,
                2 => 1.5,
                3 => 1.25,
                _ => 1.1,
            };
            body = body
                .text_size(font_size * scale)
                .font_weight(FontWeight::BOLD)
                .child(render_inline(&block.content, selection, cx));
        }
        Kind::Mermaid(Ok(graph)) => {
            body = body.pr(px(36.)).child(
                div()
                    .id(("mermaid-diagram", index))
                    .debug_selector(move || format!("mermaid-diagram-{index}"))
                    .w_full()
                    .child(super::mermaid_view::render(
                        graph.clone(),
                        font_size,
                        window,
                        cx,
                    )),
            );
        }
        Kind::Code | Kind::Mermaid(Err(_)) => {
            body = body
                .p_3()
                .pr(px(48.))
                .min_h(px(39.))
                .rounded_md()
                .bg(cx.theme().accent)
                .child(render_text(
                    block.content.text.trim_end_matches('\n'),
                    &[],
                    block.content.offset,
                    true,
                    selection,
                    cx,
                ));
            if let Kind::Mermaid(Err(error)) = &block.kind {
                body = body.child(
                    div()
                        .mt_2()
                        .text_size(px(theme::UI_FONT_SIZE))
                        .text_color(cx.theme().warning)
                        .child(*error),
                );
            }
        }
        Kind::TableRow {
            header, alignment, ..
        } => {
            let mut row = div()
                .id(("reading-table-row", index))
                .debug_selector(move || format!("reading-table-row-{index}"))
                .h_flex()
                .items_stretch()
                .w_full()
                .border_x_1()
                .border_b_1()
                .border_color(cx.theme().border)
                .when(*header, |d| {
                    d.border_t_1()
                        .bg(cx.theme().accent)
                        .font_weight(FontWeight::BOLD)
                });
            for (column, cell) in block.cells.iter().enumerate() {
                row = row.child(
                    div()
                        .id(("reading-table-cell", column))
                        .debug_selector(move || format!("reading-table-cell-{index}-{column}"))
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .p_2()
                        .when(column > 0, |d| d.border_l_1())
                        .border_color(cx.theme().border)
                        .when(alignment.get(column) == Some(&Align::Right), |d| {
                            d.text_right()
                        })
                        .when(alignment.get(column) == Some(&Align::Center), |d| {
                            d.text_center()
                        })
                        .when(!cell.images.is_empty(), |d| d.v_flex())
                        .child(render_inline(cell, selection, cx))
                        .children(
                            cell.images
                                .iter()
                                .map(|image| render_image(image, directory)),
                        ),
                );
            }
            body = body.child(row);
        }
        Kind::Paragraph => {
            body = body.child(render_inline(&block.content, selection, cx));
        }
    }
    body = body.children(
        block
            .content
            .images
            .iter()
            .map(|image| render_image(image, directory)),
    );
    if task == Some(true) {
        body = body.text_color(cx.theme().muted_foreground);
    }
    let row = div()
        .h_flex()
        .items_start()
        .gap(if task.is_some() {
            font_size * 0.5
        } else {
            px(4.)
        })
        .when(!block.prefix.is_empty(), |d| {
            if let Some(checked) = task {
                d.child(render_task_marker(
                    block, checked, index, font_size, selection, view, cx,
                ))
            } else {
                d.child(div().flex_shrink_0().child(render_text(
                    &block.prefix,
                    &[],
                    block.offset,
                    false,
                    selection,
                    cx,
                )))
            }
        })
        .child(body);
    let (top, bottom) = match block.kind {
        Kind::TableRow { header, last, .. } => (
            if header { px(8.) } else { px(0.) },
            if last { px(8.) } else { px(0.) },
        ),
        _ if task.is_some() => (font_size * 0.2, font_size * 0.2),
        _ if block.prefix.is_empty() => (px(8.), px(8.)),
        _ => (px(2.), px(2.)),
    };
    div()
        .id(("reading-block", index))
        .debug_selector(move || format!("reading-block-{index}"))
        .px_4()
        .pt(top)
        .pb(bottom)
        .w_full()
        .child(
            div()
                .pl(px((block.indent.min(32) + block.quote.min(32)) as f32 * 18.))
                .when(block.quote > 0, |d| {
                    d.border_l_2()
                        .border_color(cx.theme().border)
                        .text_color(cx.theme().muted_foreground)
                })
                .child(row),
        )
        .into_any_element()
}

fn web_url(target: &str) -> bool {
    target
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
        || target
            .get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("http://"))
}

#[cfg(test)]
mod tests {
    use super::{Copy, InputState, SelectAll, TextMode};
    use gpui::{AppContext, MouseButton, point, px};

    #[gpui::test]
    fn task_clicks_preserve_source_and_only_task_undo_is_allowed_in_reading(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let source = "앞🙂 [ ] 일반 텍스트\r\n\r\n- [ ] 한글 **항목**\r\n  - [X] 하위 완료\r\n\r\n```\r\n- [ ] 코드\r\n```\r\n";
        let window = cx.add_window(|w, cx| {
            let mut state = InputState::new(w, cx).default_value("이전 일반 편집".into());
            state.replace_all(source, w, cx);
            state.set_selected_range(0.."앞🙂".len(), cx);
            state.set_read_only(true, cx);
            state.focus(w, cx);
            state
        });
        let editor = window.root(cx).unwrap();
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let draw = |v: &mut gpui::VisualTestContext| {
            v.update(|w, cx| {
                w.refresh();
                w.draw(cx).clear(cx);
            })
        };
        draw(&mut visual);
        let revision = editor.read_with(cx, |s, _| s.revision);
        let checkbox = visual.debug_bounds("reading-task-1").unwrap().center();
        visual.simulate_click(checkbox, Default::default());
        draw(&mut visual);
        let checked = source.replacen("- [ ] 한글", "- [x] 한글", 1);
        editor.read_with(cx, |s, _| {
            assert_eq!(s.value(), checked);
            assert_eq!(s.selection, 0.."앞🙂".len());
            assert!(s.is_read_only());
            assert!(s.reading.drag.is_none());
            assert!(s.reading.anchor == s.reading.head);
        });
        visual.update(|w, cx| {
            editor.update(cx, |s, cx| {
                assert!(!s.toggle_reading_task(1, revision, w, cx));
                assert_eq!(s.value(), checked);
            })
        });
        visual.update(|w, cx| w.dispatch_action(Box::new(super::Undo), cx));
        draw(&mut visual);
        editor.read_with(cx, |s, _| assert_eq!(s.value(), source));
        visual.update(|w, cx| w.dispatch_action(Box::new(super::Undo), cx));
        editor.read_with(cx, |s, _| assert_eq!(s.value(), source)); // General edit stays blocked.
        visual.update(|w, cx| w.dispatch_action(Box::new(super::Redo), cx));
        draw(&mut visual);
        editor.read_with(cx, |s, _| assert_eq!(s.value(), checked));
        let child = visual.debug_bounds("reading-task-2").unwrap().center();
        visual.simulate_click(child, Default::default());
        draw(&mut visual);
        editor.read_with(cx, |s, _| {
            assert_eq!(s.value(), checked.replace("[X]", "[ ]"))
        });
        visual.update(|w, cx| w.dispatch_action(Box::new(super::Undo), cx));
        draw(&mut visual);
        editor.read_with(cx, |s, _| assert_eq!(s.value(), checked));
        assert!(visual.debug_bounds("reading-task-3").is_none()); // Fenced code is inert.
        let checkbox = visual.debug_bounds("reading-task-1").unwrap().center();
        visual.simulate_click(checkbox, Default::default());
        draw(&mut visual);
        editor.read_with(cx, |s, _| assert_eq!(s.value(), source));
        visual.update(|w, cx| w.dispatch_action(Box::new(SelectAll), cx));
        visual.update(|w, cx| w.dispatch_action(Box::new(Copy), cx));
        cx.update(|cx| {
            assert!(
                cx.read_from_clipboard()
                    .unwrap()
                    .text()
                    .unwrap()
                    .contains("☐ 한글 항목")
            )
        });
    }

    #[gpui::test]
    fn task_toggle_keeps_scrolled_position_through_background_reparse(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let source = "긴 한글 본문입니다.\r\n\r\n".repeat(4000) + "- [ ] 마지막 항목\r\n";
        let window = cx.add_window(|w, cx| {
            let mut state = InputState::new(w, cx).default_value(source.clone());
            state.set_read_only(true, cx);
            state.focus(w, cx);
            state
        });
        let editor = window.root(cx).unwrap();
        cx.run_until_parked();
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let draw = |v: &mut gpui::VisualTestContext| {
            v.update(|w, cx| {
                w.refresh();
                w.draw(cx).clear(cx);
            })
        };
        visual.update(|_, cx| {
            editor.update(cx, |s, cx| {
                s.reading.list.scroll_to_end();
                cx.notify();
            })
        });
        draw(&mut visual);
        let checkbox = visual.debug_bounds("reading-task-4000").unwrap().center();
        let offset = editor.read_with(cx, |s, _| s.reading.list.logical_scroll_top());
        visual.simulate_click(checkbox, Default::default());
        cx.run_until_parked();
        draw(&mut visual);
        assert!(visual.debug_bounds("reading-task-4000").is_some());
        editor.read_with(cx, |s, _| {
            assert_eq!(
                s.value(),
                source.replacen("- [ ] 마지막", "- [x] 마지막", 1)
            );
            assert_eq!(s.reading.list.logical_scroll_top().item_ix, offset.item_ix);
            assert_eq!(s.selection, 0..0);
        });
    }

    #[gpui::test]
    fn code_buttons_copy_only_the_target_block_without_changing_selection(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let source = "# 제목\n\n```rust\n\tlet 한글 = \"🙂\";\n\n```\n\n    indented\tcode\n\n```mermaid\nflowchart LR\nA-->B\n```\n\n```mermaid\nsequenceDiagram\nA->>B: hello\n```";
        let window = cx.add_window(|w, cx| {
            let mut state = InputState::new(w, cx).default_value(source.into());
            state.set_selected_range(0..2, cx);
            state.set_read_only(true, cx);
            state.reading.anchor = 0;
            state.reading.head = 3;
            state
        });
        let editor = window.root(cx).unwrap();
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        cx.simulate_window_resize(window.into(), gpui::size(px(900.), px(900.)));
        let draw = |visual: &mut gpui::VisualTestContext| {
            visual.update(|w, cx| {
                w.refresh();
                w.draw(cx).clear(cx);
            });
        };
        let revision = editor.read_with(cx, |s, _| s.revision);
        draw(&mut visual);
        assert!(visual.debug_bounds("reading-code-copy-0").is_none());
        for (index, selector, expected) in [
            (1, "reading-code-copy-1", "\tlet 한글 = \"🙂\";\n\n"),
            (2, "reading-code-copy-2", "indented\tcode\n"),
            (3, "reading-code-copy-3", "flowchart LR\nA-->B\n"),
            (4, "reading-code-copy-4", "sequenceDiagram\nA->>B: hello\n"),
        ] {
            draw(&mut visual);
            let bounds = visual.debug_bounds(selector).unwrap();
            visual.simulate_mouse_move(bounds.center(), None, Default::default());
            draw(&mut visual);
            visual.simulate_mouse_down(bounds.center(), MouseButton::Left, Default::default());
            visual.simulate_mouse_up(bounds.center(), MouseButton::Left, Default::default());
            editor.read_with(cx, |s, cx| {
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), expected);
                assert_eq!(s.value(), source);
                assert_eq!(s.revision, revision);
                assert_eq!(s.selection, 0..2);
                assert_eq!((s.reading.anchor, s.reading.head), (0, 3));
                assert!(s.reading.drag.is_none());
                assert_eq!(s.reading.copied_block, Some(index));
            });
        }
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(1500));
        cx.run_until_parked();
        editor.read_with(cx, |s, _| {
            assert!(s.reading.copied_block.is_none());
            assert!(s.reading.copy_feedback.is_none());
        });
        visual.update(|w, cx| {
            editor.update(cx, |s, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string("preserve".into()));
                s.set_value("```\nnew content\n```", w, cx);
                s.copy_reading_block(0, revision, cx);
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().unwrap(),
                    "preserve"
                );
            });
        });
    }

    #[gpui::test]
    fn images_hide_descriptions_until_loading_fails(cx: &mut gpui::TestAppContext) {
        cx.update(crate::widgets::init);
        let window = cx.add_window(|w, cx| {
            let mut state = InputState::new(w, cx)
                .source_directory(std::env::current_dir().unwrap())
                .default_value("![스크린샷 **설명**](assets/app-icon/edot.png)".into());
            state.set_read_only(true, cx);
            state
        });
        let editor = window.root(cx).unwrap();
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let draw = |visual: &mut gpui::VisualTestContext| {
            visual.update(|w, cx| {
                w.refresh();
                w.draw(cx).clear(cx);
            });
        };
        draw(&mut visual);
        cx.run_until_parked();
        draw(&mut visual);
        assert!(visual.debug_bounds("reading-image-alt").is_none());
        assert!(visual.debug_bounds("reading-image-fallback").is_none());
        let block = visual.debug_bounds("reading-block-0").unwrap();
        assert_eq!(
            block.size.height,
            px(256.),
            "image-only paragraphs must have no caption row"
        );
        editor.read_with(cx, |state, _| {
            assert_eq!(
                state.reading.document.blocks[0].content.images[0].alt,
                "스크린샷 설명"
            );
            assert!(state.reading.document.text().is_empty());
            assert!(state.reading.layouts.borrow().is_empty());
        });
        visual.update(|w, cx| {
            editor.update(cx, |state, cx| {
                state.set_value("![없는 이미지](images/edot-test-missing-image.png)", w, cx)
            });
        });
        draw(&mut visual);
        cx.run_until_parked();
        draw(&mut visual);
        assert!(visual.debug_bounds("reading-image-alt").is_some());
        assert!(visual.debug_bounds("reading-image-fallback").is_some());
        visual.update(|w, cx| {
            editor.update(cx, |state, cx| {
                state.set_value("![](images/edot-test-missing-image.png)", w, cx)
            });
        });
        draw(&mut visual);
        assert!(visual.debug_bounds("reading-image-alt").is_none());
        assert!(visual.debug_bounds("reading-image-fallback").is_some());
        visual.update(|w, cx| {
            editor.update(cx, |state, cx| {
                state.set_value("![지원하지 않는 이미지](blob:example)", w, cx)
            });
        });
        draw(&mut visual);
        cx.run_until_parked();
        draw(&mut visual);
        assert!(visual.debug_bounds("reading-image-alt").is_some());
        visual.update(|w, cx| {
            editor.update(cx, |state, cx| {
                state.set_value(
                    "| 이미지 |\n| --- |\n| ![설명](assets/app-icon/edot.png) |",
                    w,
                    cx,
                )
            });
        });
        draw(&mut visual);
        assert!(visual.debug_bounds("reading-image-alt").is_none());
        assert!(visual.debug_bounds("reading-image-fallback").is_none());
        let cell = visual.debug_bounds("reading-table-cell-1-0").unwrap();
        assert!(cell.size.height >= px(240.));
    }

    #[gpui::test]
    fn tables_keep_rows_connected_columns_aligned_and_hits_on_painted_text(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let source = format!(
            "| 이름 | 가운데 | 오른쪽 |\n| :--- | :---: | ---: |\n| 짧게 | 한글 | [문서](https://example.org/docs) |\n| {} | | right aligned text that wraps onto several lines |\n\n표 다음 문단\n",
            "긴 내용이 줄 바꿈됩니다. ".repeat(12)
        );
        let window = cx.add_window(|w, cx| {
            let mut state = InputState::new(w, cx)
                .soft_wrap(true)
                .default_value(source.clone());
            state.set_read_only(true, cx);
            state.focus(w, cx);
            state
        });
        let editor = window.root(cx).unwrap();
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let row_selectors = [
            "reading-table-row-0",
            "reading-table-row-1",
            "reading-table-row-2",
        ];
        let cell_selectors = [
            [
                "reading-table-cell-0-0",
                "reading-table-cell-0-1",
                "reading-table-cell-0-2",
            ],
            [
                "reading-table-cell-1-0",
                "reading-table-cell-1-1",
                "reading-table-cell-1-2",
            ],
            [
                "reading-table-cell-2-0",
                "reading-table-cell-2-1",
                "reading-table-cell-2-2",
            ],
        ];
        for width in [px(900.), px(420.)] {
            cx.simulate_window_resize(window.into(), gpui::size(width, px(900.)));
            visual.update(|w, cx| {
                w.refresh();
                w.draw(cx).clear(cx);
            });
            let rows = row_selectors.map(|s| visual.debug_bounds(s).unwrap());
            let cells = cell_selectors.map(|row| row.map(|s| visual.debug_bounds(s).unwrap()));
            for pair in rows.windows(2) {
                assert!(
                    (pair[0].bottom() - pair[1].top()).abs() < px(1.),
                    "table rows must touch"
                );
            }
            for row in cells.iter().skip(1) {
                for (header, cell) in cells[0].iter().zip(row) {
                    assert!((header.left() - cell.left()).abs() < px(1.));
                    assert!((header.right() - cell.right()).abs() < px(1.));
                }
            }
            assert!(
                rows[2].size.height > rows[1].size.height,
                "long cells must wrap and grow the row"
            );
            let mut link = None;
            for column in [1, 2] {
                let position = editor.read_with(cx, |s, _| {
                    let cell = &s.reading.document.blocks[1].cells[column];
                    let layouts = s.reading.layouts.borrow();
                    let f = layouts.iter().find(|f| f.offset == cell.offset).unwrap();
                    let text_width = f.layout.line_layouts()[0].unwrapped_layout.width;
                    let bounds = cells[1][column];
                    let x = if column == 1 {
                        bounds.center().x - text_width / 2.
                    } else {
                        bounds.right() - px(8.) - text_width
                    };
                    let position = point(
                        x + px(2.),
                        bounds.top() + px(8.) + f.layout.line_height() / 2.,
                    );
                    assert_eq!(
                        s.reading_selection().hit(position),
                        Some((cell.offset, true)),
                        "aligned glyph must hit its first character"
                    );
                    position
                });
                if column == 2 {
                    link = Some(position);
                }
            }
            let link = link.unwrap();
            editor.read_with(cx, |s, _| {
                let cell = &s.reading.document.blocks[2].cells[2];
                let layouts = s.reading.layouts.borrow();
                let f = layouts.iter().find(|f| f.offset == cell.offset).unwrap();
                let line = &f.layout.line_layouts()[0];
                if let Some(boundary) = line.wrap_boundaries.last() {
                    let glyph =
                        &line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix];
                    let width = line.unwrapped_layout.width - glyph.position.x;
                    let bounds = cells[2][2];
                    let position = point(
                        bounds.right() - px(8.) - width + px(2.),
                        bounds.top()
                            + px(8.)
                            + f.layout.line_height() * (line.wrap_boundaries.len() as f32 + 0.5),
                    );
                    assert_eq!(
                        s.reading_selection().hit(position),
                        Some((cell.offset + glyph.index, true)),
                        "wrapped alignment must also match the painted row"
                    );
                }
            });
            visual.simulate_mouse_down(link, MouseButton::Left, Default::default());
            visual.simulate_mouse_up(link, MouseButton::Left, Default::default());
            assert_eq!(cx.opened_url().as_deref(), Some("https://example.org/docs"));
            visual.update(|w, cx| {
                w.dispatch_action(Box::new(SelectAll), cx);
                w.dispatch_action(Box::new(Copy), cx);
            });
            editor.read_with(cx, |s, cx| {
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().unwrap(),
                    s.reading.document.text()
                );
                assert_eq!(s.value(), source);
            });
        }
    }

    #[gpui::test]
    fn reading_selection_copies_unicode_across_blocks_and_cells_without_editing(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let source = "# 머리\n\n앞 **한글🙂** [링크](https://example.org) 뒤\n\n- [x] 항목\n\n~~~\n\t가\t나\n~~~\n\n| A | B |\n|---|---|\n| 한 | 글 |\n\n".to_owned() + &"다음 문단\n\n".repeat(80);
        let window = cx.add_window(|w, cx| {
            InputState::new(w, cx)
                .soft_wrap(true)
                .default_value(source.clone())
        });
        let editor = window.root(cx).unwrap();
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_selected_range(2..8, cx);
                s.set_text_mode(TextMode::Markdown, cx);
                s.set_read_only(true, cx);
                s.focus(w, cx);
            })
        })
        .unwrap();
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let draw = |visual: &mut gpui::VisualTestContext| {
            visual.update(|w, cx| {
                w.refresh();
                w.draw(cx).clear(cx);
            })
        };
        draw(&mut visual);
        let position = |offset, index, cx: &gpui::TestAppContext| {
            editor.read_with(cx, |s, _| {
                let layouts = s.reading.layouts.borrow();
                let f = layouts.iter().find(|f| f.offset == offset).unwrap();
                f.layout.position_for_index(index).unwrap()
                    + point(px(0.1), f.layout.line_height() / 2.)
            })
        };
        let (paragraph, at, code, cells) = editor.read_with(cx, |s, _| {
            let p = &s.reading.document.blocks[1].content;
            let code = &s.reading.document.blocks[3].content;
            let cells = &s.reading.document.blocks[5].cells;
            (
                p.offset,
                p.text.find("한글").unwrap(),
                code.offset,
                [cells[0].offset, cells[1].offset],
            )
        });
        let drag = |from, to, visual: &mut gpui::VisualTestContext| {
            visual.simulate_mouse_down(from, MouseButton::Left, Default::default());
            visual.simulate_mouse_move(to, MouseButton::Left, Default::default());
            visual.simulate_mouse_up(to, MouseButton::Left, Default::default());
            draw(visual);
            visual.update(|w, cx| w.dispatch_action(Box::new(Copy), cx));
        };
        let start = position(paragraph, at, cx);
        let end = position(paragraph, at + "한글🙂".len(), cx);
        drag(start, end, &mut visual);
        cx.update(|cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "한글🙂"));
        drag(end, start, &mut visual);
        cx.update(|cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "한글🙂"));
        let table_end = position(cells[1], "글".len(), cx);
        drag(start, table_end, &mut visual);
        cx.update(|cx| {
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "한글🙂 링크 뒤\n\n☑ 항목\n\n\t가\t나\n\n\nA\tB\n\n한\t글"
            )
        });
        assert!(cx.opened_url().is_none());
        // Expanded code tabs must map back to the original rendered text bytes.
        let code_start = position(code, 0, cx);
        let code_end = position(code, "    가    나".len(), cx);
        drag(code_start, code_end, &mut visual);
        cx.update(|cx| {
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "\t가\t나"
            )
        });
        let (link_start, link_end) = editor.read_with(cx, |s, _| {
            let range = &s.reading.document.blocks[1].content.links[0].0;
            (range.start, range.end)
        });
        drag(
            position(paragraph, link_start, cx),
            position(paragraph, link_end, cx),
            &mut visual,
        );
        assert!(cx.opened_url().is_none());
        cx.update(|cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "링크"));
        // A click clears only the reading selection; copy without selection leaves the clipboard alone.
        visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(start, MouseButton::Left, Default::default());
        draw(&mut visual);
        visual.update(|w, cx| w.dispatch_action(Box::new(Copy), cx));
        cx.update(|cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "링크"));
        // Drag beyond the viewport and keep holding to extend the selection into virtualized blocks.
        let (bottom, before) = editor.read_with(cx, |s, _| {
            (
                s.reading.list.viewport_bounds().bottom(),
                s.reading.list.logical_scroll_top().item_ix,
            )
        });
        let below = point(start.x, bottom + px(50.));
        visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(below, MouseButton::Left, Default::default());
        for _ in 0..12 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(16));
            cx.run_until_parked();
            draw(&mut visual);
        }
        visual.simulate_mouse_up(below, MouseButton::Left, Default::default());
        editor.read_with(cx, |s, _| {
            assert!(s.reading.list.logical_scroll_top().item_ix > before);
            assert!(
                s.reading
                    .document
                    .text_range(s.reading_selection().range)
                    .contains("다음 문단")
            );
        });
        // Whole-document selection includes blocks which have never been laid out.
        visual.update(|w, cx| w.dispatch_action(Box::new(SelectAll), cx));
        draw(&mut visual);
        visual.update(|w, cx| w.dispatch_action(Box::new(Copy), cx));
        cx.update(|cx| {
            let s = editor.read(cx);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                s.reading.document.text()
            );
            assert!(s.reading.layouts.borrow().len() < s.reading.document.blocks.len());
            assert_eq!(s.selected_range(), 2..8);
            assert_eq!(s.value(), source);
        });
    }

    #[gpui::test]
    fn reading_discards_old_background_documents_and_caches_the_latest_source(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let initial = "# 이전\n\n**본문**\n\n".repeat(4000);
        let latest = "# 최신\n\n*변경됨*\n\n".repeat(4000);
        let window = cx.add_window(|w, cx| {
            InputState::new(w, cx)
                .soft_wrap(true)
                .default_value(initial)
        });
        let editor = window.root(cx).unwrap();
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_text_mode(TextMode::Markdown, cx);
                s.set_read_only(true, cx);
                assert!(s.reading.task.is_some());
                s.set_value(latest.clone(), w, cx);
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| {
            let s = editor.read(cx);
            assert_eq!(s.reading.revision, Some(s.revision));
            assert_eq!(s.reading.document.blocks[0].content.text, "최신");
            assert_eq!(s.value(), latest);
            assert!(s.reading.task.is_none());
        });
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_value("# 오래된 결과\n\n본문\n\n".repeat(4000), w, cx);
                s.ensure_reading(cx);
                assert!(s.reading.task.is_some());
                s.set_text_mode(TextMode::PlainText, cx);
                s.set_value("# 작은 문서", w, cx);
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window.into(), |_, _, cx| {
            editor.update(cx, |s, cx| {
                assert!(s.reading.task.is_none());
                assert_ne!(s.reading.revision, Some(s.revision));
                s.set_text_mode(TextMode::Markdown, cx);
                s.set_read_only(true, cx);
                assert_eq!(s.reading.document.blocks[0].content.text, "작은 문서");
                assert_eq!(s.reading.revision, Some(s.revision));
            });
        })
        .unwrap();
    }
}
