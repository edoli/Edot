//! Native vector paths and DirectWrite text; no SVG, images or font loader.
use super::mermaid::{self, Graph, Layout, Rect, Shape};
use crate::widgets::ActiveTheme;
use gpui::*;
use std::sync::Arc;

struct Text {
    lines: Vec<WrappedLine>,
    size: [f32; 2],
}
fn text(
    label: &str,
    font: &Font,
    font_size: Pixels,
    width: Pixels,
    window: &Window,
    cx: &App,
) -> Text {
    let run = TextRun {
        len: label.len(),
        font: font.clone(),
        color: cx.theme().foreground,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let lines: Vec<_> = window
        .text_system()
        .shape_text(
            label.to_owned().into(),
            font_size,
            &[run],
            Some(width),
            None,
        )
        .unwrap_or_default()
        .into_iter()
        .collect();
    let height = font_size * 1.4;
    let size = [
        lines
            .iter()
            .map(|l| f32::from(l.width()))
            .fold(0., f32::max),
        lines.iter().map(|l| f32::from(l.size(height).height)).sum(),
    ];
    Text { lines, size }
}

pub(super) fn render(
    graph: Arc<Graph>,
    font_size: Pixels,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let font = window.text_style().font();
    let font_size = font_size * 0.9;
    let unit = f32::from(font_size) / 16.;
    let width = px(240. * unit);
    let node_text: Vec<_> = graph
        .nodes
        .iter()
        .map(|n| text(&n.label, &font, font_size, width, window, cx))
        .collect();
    let edge_text: Vec<_> = graph
        .edges
        .iter()
        .map(|e| text(&e.label, &font, font_size, width, window, cx))
        .collect();
    let sizes: Vec<_> = node_text
        .iter()
        .zip(&graph.nodes)
        .map(|(t, node)| {
            let factor = if node.shape == Shape::Diamond { 2. } else { 1. };
            [
                (t.size[0] + 32. * unit).max(80. * unit) * factor,
                (t.size[1] + 24. * unit).max(48. * unit) * factor,
            ]
        })
        .collect();
    let labels: Vec<_> = edge_text
        .iter()
        .map(|t| {
            if t.lines.is_empty() {
                [0., 0.]
            } else {
                [t.size[0] + 12. * unit, t.size[1] + 8. * unit]
            }
        })
        .collect();
    let layout = Arc::new(mermaid::layout(&graph, &sizes, &labels, unit));
    let paint_layout = layout.clone();
    let paint_graph = graph.clone();
    div()
        .w_full()
        .max_w(px(layout.width))
        .aspect_ratio(layout.width / layout.height)
        .child(
            canvas(
                move |bounds, w, cx| {
                    let scale = f32::from(bounds.size.width) / layout.width;
                    if (scale - 1.).abs() < 0.0001 {
                        return (scale, node_text, edge_text);
                    }
                    let node_text = graph
                        .nodes
                        .iter()
                        .map(|n| text(&n.label, &font, font_size * scale, width * scale, w, cx))
                        .collect::<Vec<_>>();
                    let edge_text = graph
                        .edges
                        .iter()
                        .map(|e| text(&e.label, &font, font_size * scale, width * scale, w, cx))
                        .collect::<Vec<_>>();
                    (scale, node_text, edge_text)
                },
                move |bounds, (scale, node_text, edge_text), w, cx| {
                    paint(
                        &paint_graph,
                        &paint_layout,
                        bounds.origin,
                        scale,
                        font_size * scale,
                        &node_text,
                        &edge_text,
                        w,
                        cx,
                    );
                },
            )
            .size_full(),
        )
        .into_any_element()
}

fn path(
    points: &[Point<Pixels>],
    closed: bool,
    fill: bool,
    color: Hsla,
    width: Pixels,
    w: &mut Window,
) {
    let mut builder = if fill {
        PathBuilder::fill()
    } else {
        PathBuilder::stroke(width)
    };
    builder.add_polygon(points, closed);
    if let Ok(path) = builder.build() {
        w.paint_path(path, color);
    }
}
fn paint_text(
    text: &Text,
    rect: Bounds<Pixels>,
    line_height: Pixels,
    w: &mut Window,
    cx: &mut App,
) {
    let mut y = rect.top() + (rect.size.height - px(text.size[1])) / 2.;
    for line in &text.lines {
        let _ = line.paint(
            point(rect.left(), y),
            line_height,
            TextAlign::Center,
            Some(rect),
            w,
            cx,
        );
        y += line.size(line_height).height;
    }
}

// Glyphs have already been shaped at the scale used by the vector geometry.
#[allow(clippy::too_many_arguments)]
fn paint(
    graph: &Graph,
    layout: &Layout,
    origin: Point<Pixels>,
    scale: f32,
    font_size: Pixels,
    node_text: &[Text],
    edge_text: &[Text],
    w: &mut Window,
    cx: &mut App,
) {
    let colors = cx.theme().colors;
    let point_at = |p: [f32; 2]| origin + point(px(p[0] * scale), px(p[1] * scale));
    let bounds_at = |r: Rect| {
        Bounds::new(
            point_at([r.x, r.y]),
            size(px(r.width * scale), px(r.height * scale)),
        )
    };
    let stroke = px((1.3 * scale).max(0.6));
    let clip = w.content_mask().bounds;
    // Edges go behind nodes and their text.
    for (route, edge) in layout.routes.iter().zip(&graph.edges) {
        let points: Vec<_> = route.points.iter().copied().map(point_at).collect();
        let min = point(
            points.iter().map(|p| p.x).min().unwrap(),
            points.iter().map(|p| p.y).min().unwrap(),
        );
        let max = point(
            points.iter().map(|p| p.x).max().unwrap(),
            points.iter().map(|p| p.y).max().unwrap(),
        );
        let margin = px(8. * scale + 1.);
        if !Bounds::from_corners(min - point(margin, margin), max + point(margin, margin))
            .intersects(&clip)
        {
            continue;
        }
        path(&points, false, false, colors.muted_foreground, stroke, w);
        if edge.arrow {
            let tip = points[points.len() - 1];
            if let Some(&previous) = points[..points.len() - 1].iter().rev().find(|&&p| p != tip) {
                let dx = f32::from(tip.x - previous.x);
                let dy = f32::from(tip.y - previous.y);
                let length = dx.hypot(dy);
                let along = point(px(dx / length * 8. * scale), px(dy / length * 8. * scale));
                let across = point(px(-dy / length * 4. * scale), px(dx / length * 4. * scale));
                path(
                    &[tip, tip - along + across, tip - along - across],
                    true,
                    true,
                    colors.muted_foreground,
                    stroke,
                    w,
                );
            }
        }
    }
    for ((node, rect), text) in graph.nodes.iter().zip(&layout.nodes).zip(node_text) {
        let bounds = bounds_at(*rect);
        if !bounds.intersects(&clip) {
            continue;
        }
        if node.shape == Shape::Diamond {
            let center = bounds.center();
            let points = [
                point(center.x, bounds.top()),
                point(bounds.right(), center.y),
                point(center.x, bounds.bottom()),
                point(bounds.left(), center.y),
            ];
            path(&points, true, true, colors.accent, stroke, w);
            path(&points, true, false, colors.muted_foreground, stroke, w);
        } else {
            w.paint_quad(
                fill(bounds, colors.accent)
                    .border_widths(stroke)
                    .border_color(colors.muted_foreground)
                    .corner_radii(if node.shape == Shape::Rounded {
                        px(12. * scale)
                    } else {
                        px(2. * scale)
                    }),
            );
        }
        paint_text(text, bounds, font_size * 1.4, w, cx);
    }
    for ((edge, route), text) in graph.edges.iter().zip(&layout.routes).zip(edge_text) {
        if !edge.label.is_empty() {
            let bounds = bounds_at(route.label);
            if !bounds.intersects(&clip) {
                continue;
            }
            w.paint_quad(fill(bounds, colors.background).corner_radii(px(3. * scale)));
            paint_text(text, bounds, font_size * 1.4, w, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::text;
    use crate::widgets::ActiveTheme;
    use crate::widgets::input::{Copy, InputState, SelectAll, TextMode, reading_document::Kind};
    use gpui::{AppContext, px};
    use std::sync::Arc;

    #[gpui::test]
    fn native_diagram_keeps_source_updates_and_falls_back_for_unsupported_syntax(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::widgets::init);
        let source = "# 흐름\n\n```mermaid\nflowchart LR\n A[시작🙂] --> B{확인}\n B -->|예| C(완료)\n B -- 아니오 --> A\n```\n";
        let window = cx.add_window(|w, cx| {
            InputState::new(w, cx)
                .soft_wrap(true)
                .default_value(source.to_owned())
        });
        let editor = window.root(cx).unwrap();
        cx.update_window(window.into(), |_, w, cx| {
            editor.update(cx, |s, cx| {
                s.set_text_mode(TextMode::Markdown, cx);
                s.set_read_only(true, cx);
                s.focus(w, cx);
            });
            let text = text("한글🙂", &w.text_style().font(), px(16.), px(240.), w, cx);
            assert!(text.size[0] > 0. && text.size[1] > 0.);
            assert_eq!(text.lines[0].text.as_ref(), "한글🙂");
        })
        .unwrap();
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let draw = |visual: &mut gpui::VisualTestContext| {
            visual.update(|w, cx| {
                w.refresh();
                w.draw(cx).clear(cx);
            });
        };
        draw(&mut visual);
        let bounds = visual.debug_bounds("mermaid-diagram-1").unwrap();
        assert!(bounds.size.width > px(50.) && bounds.size.height > px(20.));
        visual.update(|w, cx| {
            let colors = cx.theme().colors;
            assert!(
                w.painted_quads()
                    .iter()
                    .filter(|q| {
                        q.background == colors.accent.into()
                            && q.border_color == colors.muted_foreground
                    })
                    .count()
                    >= 2,
                "rectangle and rounded node must actually be painted"
            );
        });
        let graph = editor.read_with(cx, |s, _| {
            assert_eq!(s.value(), source);
            let Kind::Mermaid(Ok(g)) = &s.reading.document.blocks[1].kind else {
                panic!("missing graph")
            };
            g.clone()
        });
        visual.update(|w, cx| {
            w.dispatch_action(Box::new(SelectAll), cx);
            w.dispatch_action(Box::new(Copy), cx);
        });
        cx.update(|cx| {
            assert!(
                cx.read_from_clipboard()
                    .unwrap()
                    .text()
                    .unwrap()
                    .contains("B -- 아니오 --> A")
            );
            crate::widgets::theme::Theme::update(cx, |t| {
                t.colors = crate::widgets::theme::Colors::for_appearance(true);
            });
        });
        draw(&mut visual);
        editor.read_with(cx, |s, _| {
            let Kind::Mermaid(Ok(g)) = &s.reading.document.blocks[1].kind else {
                panic!("missing dark graph")
            };
            assert!(Arc::ptr_eq(&graph, g));
        });
        visual.update(|w, cx| {
            editor.update(cx, |s, cx| {
                s.set_value(source.replace("완료", "변경됨"), w, cx)
            });
        });
        draw(&mut visual);
        editor.read_with(cx, |s, _| {
            let Kind::Mermaid(Ok(g)) = &s.reading.document.blocks[1].kind else {
                panic!("missing updated graph")
            };
            assert!(!Arc::ptr_eq(&graph, g));
            assert_eq!(g.nodes[2].label, "변경됨");
        });
        let invalid = "# 지원 범위 밖\n\n```mermaid\nsequenceDiagram\n A->>B: 한글\n```\n";
        visual.update(|w, cx| {
            editor.update(cx, |s, cx| s.set_value(invalid, w, cx));
        });
        draw(&mut visual);
        assert!(visual.debug_bounds("mermaid-diagram-1").is_none());
        editor.read_with(cx, |s, _| {
            assert_eq!(s.value(), invalid);
            assert!(matches!(
                s.reading.document.blocks[1].kind,
                Kind::Mermaid(Err(_))
            ));
            assert!(s.reading.document.text().contains("A->>B: 한글"));
        });
        visual.update(|_, cx| {
            editor.update(cx, |s, cx| s.set_text_mode(TextMode::PlainText, cx));
        });
        draw(&mut visual);
        assert!(visual.debug_bounds("markdown-reading").is_none());
        assert!(visual.debug_bounds("text-input").is_some());
    }
}
