//! A cached, virtualized GPUI reading view; source editing state stays untouched.
use super::super::scroll::AutoScroll;
use super::{markdown::*, preview_document::*, *};
use std::path::PathBuf;
use std::sync::Arc;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
struct Fragment {
    layout: TextLayout,
    offset: usize,
    tabs: Vec<(usize, usize)>,
}
impl Fragment {
    fn index(&self, position: Point<Pixels>) -> (usize, bool) {
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

pub(super) struct Preview {
    document: Arc<Document>,
    revision: Option<u64>,
    task: Option<Task<()>>,
    list: ListState,
    font: Option<(Font, Pixels)>,
    directory: PathBuf,
    anchor: usize,
    head: usize,
    drag: Option<Point<Pixels>>,
    dragged: bool,
    link: Option<String>,
    layouts: Rc<RefCell<Vec<Fragment>>>,
    auto_scroll: AutoScroll,
}
impl Default for Preview {
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
            layouts: Rc::default(),
            auto_scroll: AutoScroll::default(),
        }
    }
}
impl InputState {
    pub(super) fn stop_preview_drag(&mut self) {
        self.preview.drag = None;
        self.preview.link = None;
        self.preview.auto_scroll.stop();
    }
    pub fn is_read_only(&self) -> bool {
        self.text_mode == TextMode::MarkdownPreview
    }
    pub fn source_directory(mut self, directory: PathBuf) -> Self {
        self.preview.directory = directory;
        self
    }
    fn install_preview(&mut self, document: Document, revision: u64, cx: &mut Context<Self>) {
        self.preview.anchor = 0;
        self.preview.head = 0;
        self.preview.drag = None;
        self.preview.link = None;
        self.preview.auto_scroll.stop();
        self.preview.layouts.borrow_mut().clear();
        self.preview.document = Arc::new(document);
        self.preview.revision = Some(revision);
        self.preview.list.reset(self.preview.document.blocks.len());
        self.reveal_preview();
        cx.notify();
    }
    fn reading_selection(&self) -> ReadingSelection {
        ReadingSelection {
            layouts: self.preview.layouts.clone(),
            range: self.preview.anchor.min(self.preview.head)
                ..self.preview.anchor.max(self.preview.head),
        }
    }
    fn preview_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(start) = self.preview.drag else {
            return;
        };
        if !self.is_read_only() {
            self.preview.drag = None;
            self.preview.auto_scroll.stop();
            return;
        }
        let previous = (self.preview.dragged, self.preview.head);
        self.preview.dragged |=
            (position.x - start.x).abs() > px(3.) || (position.y - start.y).abs() > px(3.);
        if self.preview.dragged {
            if let Some((index, _)) = self.reading_selection().hit(position) {
                self.preview.head = index.min(self.preview.document.len);
            }
            if previous != (self.preview.dragged, self.preview.head) {
                cx.notify();
            }
        }
    }
    pub(super) fn reveal_preview(&self) {
        if self.is_read_only() {
            let index = self
                .preview
                .document
                .blocks
                .partition_point(|b| b.source.end <= self.cursor());
            if index < self.preview.document.blocks.len() {
                self.preview.list.scroll_to_reveal_item(index);
            }
        }
    }
    pub(super) fn ensure_preview(&mut self, cx: &mut Context<Self>) {
        if !self.is_read_only() || self.preview.revision == Some(self.revision) {
            return;
        }
        if self.content.len() <= 64 * 1024 {
            self.install_preview(
                super::preview_document::parse(&self.content),
                self.revision,
                cx,
            );
            return;
        }
        if self.preview.task.is_some() {
            return;
        }
        let mut snapshot = (self.revision, self.content.clone());
        self.preview.task = Some(cx.spawn(async move |this, cx| {
            loop {
                let (revision, content) = snapshot;
                let document = cx
                    .background_executor()
                    .spawn(async move { super::preview_document::parse(&content) })
                    .await;
                let Ok(next) = this.update(cx, |s, cx| {
                    if s.is_read_only() && s.revision == revision {
                        s.install_preview(document, revision, cx);
                    } else if s.is_read_only() && s.preview.revision != Some(s.revision) {
                        return Some((s.revision, s.content.clone()));
                    }
                    s.preview.task = None;
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
    pub(super) fn preview_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_preview(cx);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let font = (style.font(), font_size);
        if self.preview.font.as_ref() != Some(&font) {
            let offset = self.preview.list.logical_scroll_top();
            self.preview.list.reset(self.preview.document.blocks.len());
            self.preview.list.scroll_to(offset);
            self.preview.font = Some(font);
        }
        let document = self.preview.document.clone();
        let directory = self.preview.directory.clone();
        self.preview.layouts.borrow_mut().clear();
        let selection = self.reading_selection();
        let move_entity = cx.entity();
        let up_entity = cx.entity();
        let reading = div()
            .id("markdown-preview")
            .debug_selector(|| "markdown-preview".into())
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
                            s.preview.anchor = index;
                        }
                        s.preview.head = index;
                        s.preview.drag = Some(e.position);
                        s.preview.dragged = e.modifiers.shift;
                        s.preview.link = None;
                        if exact && !e.modifiers.shift {
                            let block = s
                                .preview
                                .document
                                .blocks
                                .partition_point(|b| b.offset <= index)
                                .saturating_sub(1);
                            if let Some(block) = s.preview.document.blocks.get(block) {
                                s.preview.link = std::iter::once(&block.content)
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
                if s.preview.revision != Some(s.revision) {
                    return;
                }
                let text = s.preview.document.text_range(s.reading_selection().range);
                if !text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }))
            .on_action(cx.listener(|s, _: &SelectAll, _, cx| {
                if s.preview.revision != Some(s.revision) {
                    return;
                }
                s.preview.anchor = 0;
                s.preview.head = s.preview.document.len;
                s.preview.link = None;
                cx.notify();
            }))
            .on_key_down(cx.listener(|s, e: &KeyDownEvent, w, cx| {
                match e.keystroke.key.as_str() {
                    "up" => s.preview.list.scroll_by(-px(36.)),
                    "down" => s.preview.list.scroll_by(px(36.)),
                    "pageup" => s.preview.list.scroll_by(-w.viewport_size().height * 0.8),
                    "pagedown" | "space" => {
                        s.preview.list.scroll_by(w.viewport_size().height * 0.8)
                    }
                    "home" => s.preview.list.scroll_to(ListOffset::default()),
                    "end" => s.preview.list.scroll_to_end(),
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }));
        if self.preview.revision != Some(self.revision) {
            return reading
                .child(
                    div()
                        .p_4()
                        .text_color(cx.theme().muted_foreground)
                        .child("미리보기 준비 중…"),
                )
                .into_any_element();
        }
        reading
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, cx| {
                        move_entity.update(cx, |s, cx| {
                            if let Some(position) = s.preview.auto_scroll.last_drag_position {
                                s.preview_drag(position, cx);
                            }
                        });
                        let entity = move_entity.clone();
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            entity.update(cx, |s, cx| {
                                if s.preview.drag.is_none() {
                                    return;
                                }
                                if e.pressed_button != Some(MouseButton::Left) {
                                    s.preview.drag = None;
                                    s.preview.link = None;
                                    s.preview.auto_scroll.stop();
                                    return;
                                }
                                s.preview_drag(e.position, cx);
                                s.preview.auto_scroll.last_drag_position = Some(e.position);
                                let delta = s
                                    .preview
                                    .dragged
                                    .then(|| {
                                        AutoScroll::compute_delta(
                                            e.position.y,
                                            s.preview.list.viewport_bounds(),
                                        )
                                    })
                                    .flatten();
                                s.preview.auto_scroll.set(delta, cx, |delta, s, cx| {
                                    if !s.is_read_only() || s.preview.drag.is_none() {
                                        s.preview.auto_scroll.stop();
                                        return;
                                    }
                                    s.preview.list.scroll_by(delta);
                                    cx.notify();
                                });
                            });
                        });
                        let entity = up_entity.clone();
                        window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture || e.button != MouseButton::Left {
                                return;
                            }
                            entity.update(cx, |s, cx| {
                                if s.preview.drag.is_none() {
                                    return;
                                }
                                s.preview_drag(e.position, cx);
                                s.preview.drag = None;
                                s.preview.auto_scroll.stop();
                                if let Some(target) = s.preview.link.take()
                                    && !s.preview.dragged
                                    && s.preview.anchor == s.preview.head
                                    && (web_url(&target)
                                        || target
                                            .get(..7)
                                            .is_some_and(|s| s.eq_ignore_ascii_case("mailto:")))
                                {
                                    cx.open_url(&target);
                                }
                                cx.notify();
                            });
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                list(self.preview.list.clone(), move |i, _, cx| {
                    render_block(
                        &document.blocks[i],
                        i,
                        font_size,
                        &directory,
                        &selection,
                        cx,
                    )
                })
                .size_full(),
            )
            .child(super::super::scroll::Scrollbar::vertical_list(
                &self.preview.list,
            ))
            .into_any_element()
    }
}

fn render_inline(inline: &Inline, selection: &ReadingSelection, cx: &App) -> AnyElement {
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
    };
    SelectableText {
        text,
        fragment,
        layouts: selection.layouts.clone(),
    }
    .into_any_element()
}

fn render_block(
    block: &Block,
    index: usize,
    font_size: Pixels,
    directory: &std::path::Path,
    selection: &ReadingSelection,
    cx: &App,
) -> AnyElement {
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
        Kind::Code => {
            body = body
                .p_3()
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
        }
        Kind::TableRow { header, alignment } => {
            let mut row = div()
                .h_flex()
                .items_stretch()
                .w_full()
                .border_b_1()
                .border_color(cx.theme().border)
                .when(*header, |d| {
                    d.bg(cx.theme().accent).font_weight(FontWeight::BOLD)
                });
            for (column, cell) in block.cells.iter().enumerate() {
                row = row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .p_2()
                        .border_l_1()
                        .border_color(cx.theme().border)
                        .when(alignment.get(column) == Some(&Align::Right), |d| {
                            d.text_right()
                        })
                        .when(alignment.get(column) == Some(&Align::Center), |d| {
                            d.text_center()
                        })
                        .child(render_inline(cell, selection, cx)),
                );
            }
            body = body.child(row);
        }
        Kind::Paragraph => {
            body = body.child(render_inline(&block.content, selection, cx));
        }
    }
    for source in &block.content.images {
        let image: ImageSource = if web_url(source) {
            source.clone().into()
        } else if source.contains("://") || source.starts_with("data:") {
            continue;
        } else {
            directory.join(source).into()
        };
        body = body.child(
            img(image)
                .w_full()
                .h(px(240.))
                .object_fit(ObjectFit::Contain)
                .with_fallback(|| {
                    div()
                        .child("이미지를 불러올 수 없습니다")
                        .into_any_element()
                }),
        );
    }
    let row = div()
        .h_flex()
        .items_start()
        .gap_2()
        .when(!block.prefix.is_empty(), |d| {
            d.child(div().min_w(px(24.)).child(render_text(
                &block.prefix,
                &[],
                block.offset,
                false,
                selection,
                cx,
            )))
        })
        .child(body);
    div()
        .id(("preview-block", index))
        .debug_selector(move || format!("preview-block-{index}"))
        .px_4()
        .py_2()
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
    fn preview_selection_copies_unicode_across_blocks_and_cells_without_editing(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let source = "# 머리\n\n앞 **한글🙂** [링크](https://example.org) 뒤\n\n- 항목\n\n~~~\n\t가\t나\n~~~\n\n| A | B |\n|---|---|\n| 한 | 글 |\n\n".to_owned() + &"다음 문단\n\n".repeat(80);
        let window = cx.add_window(|w, cx| {
            InputState::new(w, cx)
                .soft_wrap(true)
                .default_value(source.clone())
        });
        let editor = window.root(cx).unwrap();
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_selected_range(2..8, cx);
                s.set_text_mode(TextMode::MarkdownPreview, cx);
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
                let layouts = s.preview.layouts.borrow();
                let f = layouts.iter().find(|f| f.offset == offset).unwrap();
                f.layout.position_for_index(index).unwrap()
                    + point(px(0.1), f.layout.line_height() / 2.)
            })
        };
        let (paragraph, at, code, cells) = editor.read_with(cx, |s, _| {
            let p = &s.preview.document.blocks[1].content;
            let code = &s.preview.document.blocks[3].content;
            let cells = &s.preview.document.blocks[5].cells;
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
                "한글🙂 링크 뒤\n\n• 항목\n\n\t가\t나\n\n\nA\tB\n\n한\t글"
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
            let range = &s.preview.document.blocks[1].content.links[0].0;
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
                s.preview.list.viewport_bounds().bottom(),
                s.preview.list.logical_scroll_top().item_ix,
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
            assert!(s.preview.list.logical_scroll_top().item_ix > before);
            assert!(
                s.preview
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
                s.preview.document.text()
            );
            assert!(s.preview.layouts.borrow().len() < s.preview.document.blocks.len());
            assert_eq!(s.selected_range(), 2..8);
            assert_eq!(s.value(), source);
        });
    }

    #[gpui::test]
    fn preview_discards_old_background_documents_and_caches_the_latest_source(
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
                s.set_text_mode(TextMode::MarkdownPreview, cx);
                assert!(s.preview.task.is_some());
                s.set_value(latest.clone(), w, cx);
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| {
            let s = editor.read(cx);
            assert_eq!(s.preview.revision, Some(s.revision));
            assert_eq!(s.preview.document.blocks[0].content.text, "최신");
            assert_eq!(s.value(), latest);
            assert!(s.preview.task.is_none());
        });
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_value("# 오래된 결과\n\n본문\n\n".repeat(4000), w, cx);
                s.ensure_preview(cx);
                assert!(s.preview.task.is_some());
                s.set_text_mode(TextMode::PlainText, cx);
                s.set_value("# 작은 문서", w, cx);
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window.into(), |_, _, cx| {
            editor.update(cx, |s, cx| {
                assert!(s.preview.task.is_none());
                assert_ne!(s.preview.revision, Some(s.revision));
                s.set_text_mode(TextMode::MarkdownPreview, cx);
                assert_eq!(s.preview.document.blocks[0].content.text, "작은 문서");
                assert_eq!(s.preview.revision, Some(s.revision));
            });
        })
        .unwrap();
    }
}
