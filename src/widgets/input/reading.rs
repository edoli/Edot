//! Visual components for Markdown blocks embedded in the editor.
use super::{
    markdown::*,
    reading_document::*,
    reading_geometry::{BlockGeometry, Fragment, Projection},
    *,
};
use std::path::Path;
use std::rc::Rc;

pub(super) struct BlockView<'a> {
    pub(super) source: usize,
    pub(super) index: usize,
    pub(super) font_size: Pixels,
    pub(super) directory: &'a Path,
    pub(super) editor: WeakEntity<InputState>,
    pub(super) revision: u64,
    pub(super) copied: bool,
    pub(super) raw: &'a str,
    pub(super) document: &'a super::visual::VisualDocument,
    pub(super) revealed: &'a [usize],
    pub(super) selected: &'a [Range<usize>],
    pub(super) marked: &'a [Range<usize>],
    pub(super) geometry: BlockGeometry,
    pub(super) wrap: bool,
}

pub(super) fn render_visual_blocks(
    blocks: &[Block],
    view: BlockView<'_>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let source = view.source;
    let body =
        div()
            .id(("visual-block", source))
            .debug_selector(move || format!("visual-block-{source}"))
            .v_flex()
            .w_full()
            .children(blocks.iter().enumerate().map(|(index, block)| {
                render_block(block, index, view.font_size, &view, window, cx)
            }));
    if !matches!(blocks[0].kind, Kind::Code | Kind::Mermaid(_)) {
        return body.into_any_element();
    }
    let BlockView {
        editor,
        revision,
        copied,
        index,
        ..
    } = view;
    let edit_editor = editor.clone();
    body.relative()
        .group("visual-code")
        .child(
            div()
                .debug_selector(move || format!("visual-code-edit-{source}"))
                .absolute()
                .top(px(14.))
                .right(px(52.))
                .opacity(0.)
                .group_hover("visual-code", |style| style.opacity(1.))
                .child(
                    Button::new(("visual-code-edit", source))
                        .small()
                        .tab_stop(false)
                        .icon(IconName::Pencil)
                        .tooltip("블록 원문 편집")
                        .bg(cx.theme().background)
                        .border_1()
                        .border_color(cx.theme().border)
                        .on_click(move |_, window, cx| {
                            let _ = edit_editor.update(cx, |state, cx| {
                                state.start_visual_block_edit(index, revision, window, cx);
                            });
                        }),
                ),
        )
        .child(
            div()
                .debug_selector(move || format!("visual-code-copy-{source}"))
                .absolute()
                .top(px(14.))
                .right(px(22.))
                .opacity(if copied { 1. } else { 0. })
                .group_hover("visual-code", |style| style.opacity(1.))
                .child(
                    Button::new(("visual-code-copy", source))
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
                            let _ = editor.update(cx, |state, cx| {
                                state.copy_visual_code(index, revision, cx);
                            });
                        }),
                ),
        )
        .into_any_element()
}

fn render_inline(inline: &Inline, view: &BlockView<'_>, cx: &App) -> AnyElement {
    if inline.text.is_empty() && !inline.images.is_empty() {
        return div().into_any_element();
    }
    render_mapped_text(inline, view, false, cx)
}

fn render_mapped_text(inline: &Inline, view: &BlockView<'_>, code: bool, cx: &App) -> AnyElement {
    let markers = if code {
        Vec::new()
    } else {
        view.document.reveal_markers(view.revealed, &inline.source)
    };
    let projection = Rc::new(Projection::new(inline, view.raw, &markers, code));
    let spans: Vec<_> = if code {
        Vec::new()
    } else {
        inline
            .spans
            .iter()
            .map(|span| (projection.styled_range(&span.range), span.style))
            .collect()
    };
    let map_ranges = |ranges: &[Range<usize>]| -> Vec<_> {
        ranges
            .iter()
            .filter(|range| {
                !range.is_empty()
                    && range.start < inline.source.end
                    && range.end > inline.source.start
            })
            .filter_map(|range| {
                let start = projection.source_to_display(range.start);
                let end = projection.source_to_display(range.end);
                (start < end
                    && projection.text.is_char_boundary(start)
                    && projection.text.is_char_boundary(end))
                .then_some(start..end)
            })
            .collect()
    };
    let selected = map_ranges(view.selected);
    let marked = map_ranges(view.marked);
    let mut edges = vec![0, projection.text.len()];
    for range in spans
        .iter()
        .map(|(range, _)| range)
        .chain(&selected)
        .chain(&marked)
    {
        edges.extend([range.start, range.end]);
    }
    edges.sort_unstable();
    edges.dedup();
    let highlights = edges.windows(2).filter_map(|pair| {
        let range = pair[0]..pair[1];
        if !projection.text.is_char_boundary(range.start)
            || !projection.text.is_char_boundary(range.end)
        {
            return None;
        }
        let flags = spans
            .iter()
            .find(|(span, _)| span.contains(&range.start))
            .map_or(0, |(_, flags)| *flags);
        let mut style = highlight_style(flags, cx);
        if selected
            .iter()
            .any(|selection| selection.contains(&range.start))
        {
            style.background_color = Some(cx.theme().selection);
        }
        if marked.iter().any(|mark| mark.contains(&range.start)) {
            style.underline = Some(UnderlineStyle {
                thickness: px(1.),
                color: Some(cx.theme().foreground),
                wavy: false,
            });
        }
        Some((range, style))
    });
    let text = StyledText::new(projection.text.clone()).with_highlights(highlights);
    let mut links: Vec<_> = inline
        .links
        .iter()
        .map(|(range, url)| {
            (
                projection.styled_range(range),
                SharedString::from(url.clone()),
            )
        })
        .collect();
    if !code {
        links.extend(
            super::links::ranges(&projection.text)
                .into_iter()
                .map(|range| {
                    let target = SharedString::from(projection.text[range.clone()].to_owned());
                    (range, target)
                }),
        );
    }
    RichText {
        text,
        projection,
        links,
        geometry: view.geometry.clone(),
        block: view.source,
    }
    .into_any_element()
}

fn highlight_style(style: u16, cx: &App) -> HighlightStyle {
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
        let primary = cx.theme().primary;
        h.color = Some(primary);
        h.underline = Some(UnderlineStyle {
            thickness: px(1.),
            color: Some(primary),
            wavy: false,
        });
    }
    h
}

struct RichText {
    text: StyledText,
    projection: Rc<Projection>,
    links: Vec<(Range<usize>, SharedString)>,
    geometry: BlockGeometry,
    block: usize,
}

impl Element for RichText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        self.text.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.text.prepaint(id, inspector, bounds, state, window, cx);
        let fragment = Rc::new(Fragment {
            block: self.block,
            projection: self.projection.clone(),
            layout: self.text.layout().clone(),
            align: window.text_style().text_align,
            nowrap: window.text_style().white_space == WhiteSpace::Nowrap,
            links: std::mem::take(&mut self.links),
        });
        self.geometry.register(fragment);
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text
            .paint(id, inspector, bounds, state, prepaint, window, cx);
    }
}

fn render_image(image: &ReadingImage, view: &BlockView<'_>) -> AnyElement {
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
    let resource = if super::links::is_web_url(source) {
        Some(Resource::Uri(source.clone().into()))
    } else if source.contains("://") || source.starts_with("data:") {
        None
    } else {
        Some(Resource::from(view.directory.join(source)))
    };
    let body = if let Some(resource) = &resource {
        div()
            .relative()
            .w_full()
            .h(px(240.))
            .overflow_hidden()
            .child(
                img(ImageSource::Resource(resource.clone()))
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Contain)
                    .with_fallback(fallback),
            )
            .into_any_element()
    } else {
        fallback()
    };
    let start = image.source_range.start;
    let editor = view.editor.clone();
    let index = view.index;
    let revision = view.revision;
    let toolbar = canvas(
        move |bounds, window, cx| {
            let target = if let Some(resource) = &resource {
                match window.get_asset::<ImgResourceLoader>(resource, cx) {
                    Some(Ok(image)) if image.frame_count() != 0 => {
                        ObjectFit::Contain.get_bounds(bounds, image.size(0))
                    }
                    Some(_) => bounds,
                    None => return None,
                }
            } else {
                bounds
            };
            if target.size.width <= px(0.) || target.size.height <= px(0.) {
                return None;
            }
            let editor = editor.clone();
            let mut overlay = div()
                .id(("visual-image-area", start))
                .debug_selector(move || format!("visual-image-area-{start}"))
                .relative()
                .size_full()
                .group("visual-image")
                .child(
                    div()
                        .debug_selector(move || format!("visual-image-edit-{start}"))
                        .absolute()
                        .top(px(6.))
                        .right(px(6.))
                        .opacity(0.)
                        .group_hover("visual-image", |style| style.opacity(1.))
                        .child(
                            Button::new(("visual-image-edit", start))
                                .small()
                                .tab_stop(false)
                                .icon(IconName::Pencil)
                                .tooltip("이미지 원문 편집")
                                .bg(cx.theme().background)
                                .border_1()
                                .border_color(cx.theme().border)
                                .on_click(move |_, window, cx| {
                                    let _ = editor.update(cx, |state, cx| {
                                        state.start_visual_block_edit_at(
                                            index,
                                            revision,
                                            Some(start),
                                            window,
                                            cx,
                                        );
                                    });
                                }),
                        ),
                )
                .into_any_element();
            overlay.prepaint_as_root(
                target.origin,
                target.size.map(AvailableSpace::Definite),
                window,
                cx,
            );
            Some(overlay)
        },
        |_, overlay, window, cx| {
            if let Some(mut overlay) = overlay {
                overlay.paint(window, cx);
            }
        },
    )
    .absolute()
    .inset_0();
    div()
        .id(("visual-image", start))
        .relative()
        .w_full()
        .child(body)
        .child(toolbar)
        .into_any_element()
}

fn render_task_marker(
    block: &Block,
    checked: bool,
    index: usize,
    font_size: Pixels,
    view: &BlockView<'_>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let size = (font_size * 1.1).max(px(12.));
    let height = window.line_height().max(size);
    let editor = view.editor.clone();
    let revision = view.revision;
    let marker = block.task_marker;
    div()
        .id(("visual-task", index))
        .debug_selector(move || format!("visual-task-{}", marker.unwrap_or_default()))
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
        .hover(|d| d.bg(cx.theme().primary.opacity(0.1)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            if let Some(marker) = marker {
                let _ = editor.update(cx, |state, cx| {
                    if state.toggle_visual_task(marker, revision, cx) {
                        state.focus(window, cx);
                    }
                });
            }
            cx.stop_propagation();
        })
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
    view: &BlockView<'_>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let task = match block.prefix.as_str() {
        "☐" => Some(false),
        "☑" => Some(true),
        _ => None,
    };
    let mut body = div().v_flex().min_w_0().flex_1().when(
        !view.wrap && !matches!(block.kind, Kind::TableRow { .. }),
        |body| body.whitespace_nowrap(),
    );
    match &block.kind {
        Kind::Rule => {
            body = body.child(div().h(px(1.)).my_2().bg(cx.theme().border));
        }
        Kind::Heading(level) => {
            body = body
                .text_size(font_size * heading_scale(*level))
                .font_weight(FontWeight::BOLD)
                .child(render_inline(&block.content, view, cx));
        }
        Kind::Mermaid(Ok(graph)) => {
            body = body.pr(px(36.)).child(
                div()
                    .id(("mermaid-diagram", index))
                    .debug_selector(move || format!("mermaid-diagram-{index}"))
                    .w_full()
                    .child(super::mermaid_view::render(
                        graph.clone(),
                        &block.content,
                        view,
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
                .child(render_mapped_text(&block.content, view, true, cx));
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
                        .when(cell.cell.is_some(), |row| {
                            row.child(render_inline(cell, view, cx))
                        })
                        .children(cell.images.iter().map(|image| render_image(image, view))),
                );
            }
            body = body.child(row);
        }
        Kind::Paragraph => {
            body = body.child(render_inline(&block.content, view, cx));
        }
    }
    body = body.children(
        block
            .content
            .images
            .iter()
            .map(|image| render_image(image, view)),
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
                    block, checked, index, font_size, view, window, cx,
                ))
            } else {
                d.child(div().flex_shrink_0().child(block.prefix.clone()))
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

pub(super) fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.8,
        2 => 1.5,
        3 => 1.25,
        _ => 1.1,
    }
}
