//! A deliberately small flowchart grammar and deterministic layout, without GPUI.
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    Rectangle,
    Rounded,
    Diamond,
}
pub(super) struct Node {
    pub label: String,
    pub shape: Shape,
}
pub(super) struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: String,
    pub arrow: bool,
}
pub(super) struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub ranks: Vec<usize>,
    horizontal: bool,
    reverse: bool,
}

const UNSUPPORTED: &str = "지원하지 않는 Mermaid 문법입니다";
const LIMIT: &str = "flowchart는 128개 노드·256개 연결선·32KiB까지 지원합니다";

// Statements end at a newline/semicolon outside labels. Comments are recognized
// only outside brackets, quotes and edge labels, so punctuation inside text survives.
fn statements(source: &str) -> Result<Vec<&str>, &'static str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut close = None;
    let mut quoted = false;
    let mut escaped = false;
    let mut comment = false;
    for (i, c) in source.char_indices() {
        if comment {
            if c == '\n' {
                start = i + 1;
                comment = false;
            }
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && c == '\\' {
            escaped = true;
        } else if c == '"' {
            quoted = !quoted;
        } else if !quoted {
            if close == Some(c) {
                close = None;
            } else if close.is_none() {
                match c {
                    '[' => close = Some(']'),
                    '(' => close = Some(')'),
                    '{' => close = Some('}'),
                    '|' => close = Some('|'),
                    '%' if source[i..].starts_with("%%") => {
                        if source[i..].starts_with("%%{") {
                            return Err(UNSUPPORTED);
                        }
                        result.push(source[start..i].trim());
                        comment = true;
                    }
                    '\n' | ';' => {
                        result.push(source[start..i].trim());
                        start = i + c.len_utf8();
                    }
                    _ => {}
                }
            }
        }
    }
    if quoted || close.is_some() {
        return Err("Mermaid 라벨의 괄호 또는 따옴표가 닫히지 않았습니다");
    }
    if !comment {
        result.push(source[start..].trim());
    }
    result.retain(|s| !s.is_empty());
    Ok(result)
}

fn label(text: &str) -> Result<String, &'static str> {
    let text = text.trim();
    let value = if let Some(text) = text.strip_prefix('"') {
        let text = text.strip_suffix('"').ok_or(UNSUPPORTED)?;
        let mut value = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some(c @ ('"' | '\\')) => value.push(c),
                    _ => return Err(UNSUPPORTED),
                }
            } else if c == '"' {
                return Err(UNSUPPORTED);
            } else {
                value.push(c);
            }
        }
        value
    } else {
        if text.contains(['[', ']', '(', ')', '{', '}', '"']) {
            return Err(UNSUPPORTED);
        }
        text.to_owned()
    };
    // No HTML, Markdown labels or entity expansion. Unsupported syntax stays code.
    if value.contains(['<', '>', '`', '&', '\n', '\r']) || value.chars().any(char::is_control) {
        return Err(UNSUPPORTED);
    }
    Ok(value)
}

struct Parser<'a> {
    rest: &'a str,
    graph: Graph,
    ids: HashMap<String, usize>,
}
fn closing_label(source: &str, close: char) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && c == '\\' {
            escaped = true;
        } else if c == '"' {
            quoted = !quoted;
        } else if !quoted && c == close {
            return Some(i);
        }
    }
    None
}
impl Parser<'_> {
    fn node(&mut self) -> Result<usize, &'static str> {
        self.rest = self.rest.trim_start();
        let end = self
            .rest
            .char_indices()
            .take_while(|(i, c)| {
                c.is_alphanumeric()
                    || *c == '_'
                    || (*c == '-' && !self.rest[*i..].starts_with("--"))
            })
            .map(|(i, c)| i + c.len_utf8())
            .last()
            .ok_or(UNSUPPORTED)?;
        let id = &self.rest[..end];
        if matches!(
            id,
            "subgraph"
                | "end"
                | "style"
                | "classDef"
                | "class"
                | "click"
                | "linkStyle"
                | "direction"
        ) {
            return Err(UNSUPPORTED);
        }
        self.rest = self.rest[end..].trim_start();
        let declaration = match self.rest.chars().next() {
            Some(open @ ('[' | '(' | '{')) => {
                let (close, shape) = match open {
                    '[' => (']', Shape::Rectangle),
                    '(' => (')', Shape::Rounded),
                    _ => ('}', Shape::Diamond),
                };
                let end = closing_label(&self.rest[1..], close).ok_or(UNSUPPORTED)? + 1;
                let text = label(&self.rest[1..end])?;
                self.rest = self.rest[end + 1..].trim_start();
                Some((text, shape))
            }
            _ => None,
        };
        if let Some(&index) = self.ids.get(id) {
            if let Some((label, shape)) = declaration {
                self.graph.nodes[index] = Node { label, shape };
            }
            return Ok(index);
        }
        if self.graph.nodes.len() == 128 {
            return Err(LIMIT);
        }
        let (label, shape) = declaration.unwrap_or_else(|| (id.to_owned(), Shape::Rectangle));
        let index = self.graph.nodes.len();
        self.graph.nodes.push(Node { label, shape });
        self.ids.insert(id.to_owned(), index);
        Ok(index)
    }
    fn edge(&mut self) -> Result<(String, bool), &'static str> {
        let (mut text, arrow) = if let Some(rest) = self.rest.strip_prefix("-->") {
            self.rest = rest.trim_start();
            (String::new(), true)
        } else if let Some(rest) = self.rest.strip_prefix("---") {
            self.rest = rest.trim_start();
            (String::new(), false)
        } else if let Some(rest) = self.rest.strip_prefix("-- ") {
            let end = rest.find("-->").ok_or(UNSUPPORTED)?;
            let text = label(&rest[..end])?;
            self.rest = rest[end + 3..].trim_start();
            (text, true)
        } else {
            return Err(UNSUPPORTED);
        };
        if let Some(rest) = self.rest.strip_prefix('|') {
            if !text.is_empty() {
                return Err(UNSUPPORTED);
            }
            let end = closing_label(rest, '|').ok_or(UNSUPPORTED)?;
            text = label(&rest[..end])?;
            self.rest = rest[end + 1..].trim_start();
        }
        Ok((text, arrow))
    }
}

pub(super) fn parse(source: &str) -> Result<Graph, &'static str> {
    if source.len() > 32 * 1024 {
        return Err(LIMIT);
    }
    let statements = statements(source)?;
    let mut header = statements.first().ok_or(UNSUPPORTED)?.split_whitespace();
    if !matches!(header.next(), Some("flowchart" | "graph")) {
        return Err(UNSUPPORTED);
    }
    let (horizontal, reverse) = match header.next() {
        Some("TD" | "TB") => (false, false),
        Some("BT") => (false, true),
        Some("LR") => (true, false),
        Some("RL") => (true, true),
        _ => return Err(UNSUPPORTED),
    };
    if header.next().is_some() {
        return Err(UNSUPPORTED);
    }
    let mut parser = Parser {
        rest: "",
        ids: HashMap::new(),
        graph: Graph {
            nodes: Vec::new(),
            edges: Vec::new(),
            ranks: Vec::new(),
            horizontal,
            reverse,
        },
    };
    for statement in &statements[1..] {
        parser.rest = statement;
        let mut from = parser.node()?;
        while !parser.rest.is_empty() {
            let (label, arrow) = parser.edge()?;
            let to = parser.node()?;
            if parser.graph.edges.len() == 256 {
                return Err(LIMIT);
            }
            parser.graph.edges.push(Edge {
                from,
                to,
                label,
                arrow,
            });
            from = to;
        }
    }
    if parser.graph.nodes.is_empty() {
        return Err("flowchart에 노드가 없습니다");
    }
    parser.graph.ranks = ranks(&parser.graph);
    Ok(parser.graph)
}

fn ranks(graph: &Graph) -> Vec<usize> {
    let n = graph.nodes.len();
    let mut incoming = vec![0usize; n];
    let mut outgoing = vec![Vec::new(); n];
    for edge in &graph.edges {
        incoming[edge.to] += 1;
        outgoing[edge.from].push(edge.to);
    }
    let mut queue: VecDeque<_> = (0..n).filter(|&i| incoming[i] == 0).collect();
    let mut done = vec![false; n];
    let mut ranks = vec![0; n];
    let mut count = 0;
    while count < n {
        // Break a cycle in declaration order; edges to processed nodes use side lanes.
        let i = queue
            .pop_front()
            .unwrap_or_else(|| done.iter().position(|d| !d).unwrap());
        if done[i] {
            continue;
        }
        done[i] = true;
        count += 1;
        for &to in &outgoing[i] {
            if !done[to] {
                ranks[to] = ranks[to].max(ranks[i] + 1);
                incoming[to] -= 1;
                if incoming[to] == 0 {
                    queue.push_back(to);
                }
            }
        }
    }
    ranks
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
pub(super) struct Route {
    pub points: Vec<[f32; 2]>,
    pub label: Rect,
}
pub(super) struct Layout {
    pub nodes: Vec<Rect>,
    pub routes: Vec<Route>,
    pub width: f32,
    pub height: f32,
}

pub(super) fn layout(graph: &Graph, nodes: &[[f32; 2]], labels: &[[f32; 2]], unit: f32) -> Layout {
    let mut levels = vec![Vec::new(); graph.ranks.iter().max().copied().unwrap_or(0) + 1];
    for (i, &rank) in graph.ranks.iter().enumerate() {
        levels[rank].push(i);
    }
    let (main, cross) = if graph.horizontal { (0, 1) } else { (1, 0) };
    let gap = labels
        .iter()
        .map(|s| s[main] + 28. * unit)
        .fold(64. * unit, f32::max);
    let cross_gap = labels
        .iter()
        .map(|s| s[cross] * 2. + 16. * unit)
        .fold(36. * unit, f32::max);
    let widths: Vec<f32> = levels
        .iter()
        .map(|level| {
            level.iter().map(|&i| nodes[i][cross]).sum::<f32>()
                + level.len().saturating_sub(1) as f32 * cross_gap
        })
        .collect();
    let widest = widths.iter().copied().fold(0., f32::max);
    let mut boxes = vec![Rect::default(); nodes.len()];
    let mut along = 24. * unit;
    for (rank, level) in levels.iter().enumerate() {
        let depth = level.iter().map(|&i| nodes[i][main]).fold(0., f32::max);
        let mut across = 24. * unit + (widest - widths[rank]) / 2.;
        for &i in level {
            let center = along + (depth - nodes[i][main]) / 2.;
            let (x, y) = if graph.horizontal {
                (center, across)
            } else {
                (across, center)
            };
            boxes[i] = Rect {
                x,
                y,
                width: nodes[i][0],
                height: nodes[i][1],
            };
            across += nodes[i][cross] + cross_gap;
        }
        along += depth + gap;
    }
    let total_main = along - gap + 24. * unit;
    let ports = |r: Rect| -> ([f32; 2], [f32; 2]) {
        if graph.horizontal {
            (
                [r.x, r.y + r.height / 2.],
                [r.x + r.width, r.y + r.height / 2.],
            )
        } else {
            (
                [r.x + r.width / 2., r.y],
                [r.x + r.width / 2., r.y + r.height],
            )
        }
    };
    let point = |m, c| if graph.horizontal { [m, c] } else { [c, m] };
    let mut lane = 24. * unit + widest;
    let mut routes = Vec::new();
    for (i, edge) in graph.edges.iter().enumerate() {
        let start = ports(boxes[edge.from]).1;
        let end = ports(boxes[edge.to]).0;
        let points = if graph.ranks[edge.to] == graph.ranks[edge.from] + 1 {
            let middle = (start[main] + end[main]) / 2.;
            vec![
                start,
                point(middle, start[cross]),
                point(middle, end[cross]),
                end,
            ]
        } else {
            lane += (labels[i][cross] + 24. * unit).max(36. * unit);
            vec![
                start,
                point(start[main] + 16. * unit, start[cross]),
                point(start[main] + 16. * unit, lane),
                point(end[main] - 16. * unit, lane),
                point(end[main] - 16. * unit, end[cross]),
                end,
            ]
        };
        // Prefer the central cross-axis segment for branch labels.
        let middle = if points.len() == 4 && (start[cross] - end[cross]).abs() > 1. {
            [
                (points[1][0] + points[2][0]) / 2.,
                (points[1][1] + points[2][1]) / 2.,
            ]
        } else {
            let p = points.len() / 2;
            [
                (points[p - 1][0] + points[p][0]) / 2.,
                (points[p - 1][1] + points[p][1]) / 2.,
            ]
        };
        routes.push(Route {
            points,
            label: Rect {
                x: middle[0] - labels[i][0] / 2.,
                y: middle[1] - labels[i][1] / 2.,
                width: labels[i][0],
                height: labels[i][1],
            },
        });
    }
    let (width, height) = if graph.horizontal {
        (total_main, lane + 24. * unit)
    } else {
        (lane + 24. * unit, total_main)
    };
    if graph.reverse {
        for r in &mut boxes {
            if graph.horizontal {
                r.x = width - r.x - r.width;
            } else {
                r.y = height - r.y - r.height;
            }
        }
        for r in &mut routes {
            for p in &mut r.points {
                p[main] = total_main - p[main];
            }
            if graph.horizontal {
                r.label.x = width - r.label.x - r.label.width;
            } else {
                r.label.y = height - r.label.y - r.label.height;
            }
        }
    }
    // Include label extents in the viewport instead of clipping long edge labels.
    let min_x = routes.iter().map(|r| r.label.x).fold(0., f32::min);
    let min_y = routes.iter().map(|r| r.label.y).fold(0., f32::min);
    let max_x = routes
        .iter()
        .map(|r| r.label.x + r.label.width)
        .fold(width, f32::max);
    let max_y = routes
        .iter()
        .map(|r| r.label.y + r.label.height)
        .fold(height, f32::max);
    for r in &mut boxes {
        r.x -= min_x;
        r.y -= min_y;
    }
    for r in &mut routes {
        r.label.x -= min_x;
        r.label.y -= min_y;
        for p in &mut r.points {
            p[0] -= min_x;
            p[1] -= min_y;
        }
    }
    Layout {
        nodes: boxes,
        routes,
        width: max_x - min_x,
        height: max_y - min_y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grammar_preserves_labels_and_layout_handles_branches_cycles_and_directions() {
        let source = "%% 주석\nflowchart TB; A[\"시작; %% 🙂\"] --> B{확인}; B -->|\"예|확인\"| C(완료); B -- 아니오 --> A; C --> C; D[독립]";
        let graph = parse(source).unwrap();
        assert_eq!(graph.nodes.len(), 4);
        assert_eq!(graph.nodes[0].label, "시작; %% 🙂");
        assert_eq!(graph.nodes[1].shape, Shape::Diamond);
        assert_eq!(graph.nodes[2].shape, Shape::Rounded);
        assert_eq!(graph.edges[1].label, "예|확인");
        assert_eq!(graph.edges[2].label, "아니오");
        for direction in ["TB", "TD", "BT", "LR", "RL"] {
            let graph =
                parse(&source.replace("flowchart TB", &format!("graph {direction}"))).unwrap();
            let sizes = vec![[120., 60.]; graph.nodes.len()];
            let labels = vec![[60., 24.]; graph.edges.len()];
            let scene = layout(&graph, &sizes, &labels, 1.);
            assert!(scene.width > 0. && scene.height > 0.);
            for (i, a) in scene.nodes.iter().enumerate() {
                assert!(
                    a.x >= 0.
                        && a.y >= 0.
                        && a.x + a.width <= scene.width
                        && a.y + a.height <= scene.height
                );
                for b in &scene.nodes[i + 1..] {
                    assert!(
                        a.x + a.width <= b.x
                            || b.x + b.width <= a.x
                            || a.y + a.height <= b.y
                            || b.y + b.height <= a.y
                    );
                }
            }
            for r in &scene.routes {
                assert!(r.points.iter().all(|p| p[0].is_finite()
                    && p[1].is_finite()
                    && p[0] >= 0.
                    && p[1] >= 0.
                    && p[0] <= scene.width
                    && p[1] <= scene.height));
            }
        }
        for source in [
            "sequenceDiagram\nA->>B: hi",
            "flowchart TB\nsubgraph a\nA\nend",
            "flowchart TB\nA[[x]]",
            "flowchart TB\nA -->",
            "flowchart TB\nA[\"unclosed]",
            "flowchart TB\nA -.-> B",
            "flowchart TB\nA[x]:::red",
            "%%{init: {}}%%\nflowchart TB\nA --> B",
        ] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source}");
        }
        assert!(parse(&format!("flowchart TB\n{}", "A;".repeat(20000))).is_err());
        assert!(
            parse(&format!(
                "flowchart TB\n{}",
                (0..129).map(|i| format!("A{i};")).collect::<String>()
            ))
            .is_err()
        );
    }
}
