//! Inline Markdown shortcuts keep selections and use one undo transaction.
use super::*;

struct FormatEdit {
    range: Range<usize>,
    text: String,
    selection: Selection,
}

fn run_length(text: &str, byte: u8, reverse: bool) -> usize {
    if reverse {
        text.bytes().rev().take_while(|b| *b == byte).count()
    } else {
        text.bytes().take_while(|b| *b == byte).count()
    }
}

fn format_edit(content: &str, mut selection: Selection, marker: &str) -> FormatEdit {
    let range = selection.range.clone();
    let selected = &content[range.clone()];
    let before = &content[..range.start];
    let after = &content[range.end..];
    let matches = |left: &str, right: &str| {
        left.ends_with(marker)
            && right.starts_with(marker)
            && (marker != "*"
                || run_length(left, b'*', true) % 2 == 1 && run_length(right, b'*', false) % 2 == 1)
    };
    let outside_code = if marker == "`" {
        [0, 1].into_iter().find_map(|padding| {
            let left = if padding == 0 {
                before
            } else {
                before.strip_suffix(' ')?
            };
            let right = if padding == 0 {
                after
            } else {
                after.strip_prefix(' ')?
            };
            let length = run_length(left, b'`', true);
            (length > 0 && length == run_length(right, b'`', false)).then_some(length + padding)
        })
    } else {
        None
    };
    let inside_code = (marker == "`" && !selected.is_empty())
        .then(|| {
            (
                run_length(selected, b'`', false),
                run_length(selected, b'`', true),
            )
        })
        .filter(|(left, right)| *left > 0 && left == right && selected.len() >= left * 2);
    let (range, text, inner) = if outside_code.is_some() || marker != "`" && matches(before, after)
    {
        let length = outside_code.unwrap_or(marker.len());
        (
            range.start - length..range.end + length,
            selected.to_owned(),
            0..selected.len(),
        )
    } else if inside_code.is_some()
        || marker != "`" && selected.len() >= marker.len() * 2 && matches(selected, selected)
    {
        let length = inside_code.map_or(marker.len(), |(length, _)| length);
        let mut text = &selected[length..selected.len() - length];
        if marker == "`"
            && text.starts_with(' ')
            && text.ends_with(' ')
            && !text.bytes().all(|b| b == b' ')
        {
            text = &text[1..text.len() - 1];
        }
        let text = text.to_owned();
        let len = text.len();
        (range, text, 0..len)
    } else {
        let marker = if marker == "`" {
            let longest = selected
                .split(|c| c != '`')
                .map(str::len)
                .max()
                .unwrap_or(0);
            "`".repeat(longest + 1)
        } else {
            marker.to_owned()
        };
        // CommonMark requires padding when content touches a backtick delimiter.
        let pad = marker.starts_with('`')
            && (selected.starts_with('`')
                || selected.ends_with('`')
                || selected.starts_with(' ')
                    && selected.ends_with(' ')
                    && !selected.bytes().all(|b| b == b' '));
        let padding = if pad { " " } else { "" };
        let start = marker.len() + padding.len();
        let end = start + selected.len();
        (
            range,
            format!("{marker}{padding}{selected}{padding}{marker}"),
            start..end,
        )
    };
    selection.range = inner;
    selection.preferred_x = None;
    selection.after_wrap = false;
    FormatEdit {
        range,
        text,
        selection,
    }
}

impl InputState {
    pub(super) fn toggle_inline_format(&mut self, marker: &str, cx: &mut Context<Self>) {
        if !self.multiline || self.text_mode != TextMode::Markdown || self.is_read_only() {
            return;
        }
        self.finish_composition();
        self.stop_pointer_drag();
        self.deduplicate_selections();
        let mut selections = self.extra.clone();
        selections.push(Selection {
            range: self.selection.clone(),
            reversed: self.reversed,
            preferred_x: None,
            after_wrap: self.caret_after_wrap,
        });
        let mut edits: Vec<_> = selections
            .into_iter()
            .map(|selection| format_edit(&self.content, selection, marker))
            .collect();
        let mut order: Vec<_> = (0..edits.len()).collect();
        order.sort_by_key(|i| (edits[*i].range.start, edits[*i].range.end));
        // Expanded delimiters can overlap at adjacent carets; keep that command
        // unchanged rather than merging two different formatting operations.
        if order
            .windows(2)
            .any(|pair| edits[pair[0]].range.end > edits[pair[1]].range.start)
        {
            return;
        }
        let mut shift = 0isize;
        for index in order {
            let edit = &mut edits[index];
            let at = edit.range.start.checked_add_signed(shift).unwrap();
            edit.selection.range = at + edit.selection.range.start..at + edit.selection.range.end;
            shift += edit.text.len() as isize - edit.range.len() as isize;
        }
        self.commit_multiple(
            edits.iter().map(|edit| edit.range.clone()).collect(),
            edits.iter().map(|edit| edit.text.clone()).collect(),
            false,
            cx,
        );
        let primary = edits.pop().unwrap().selection;
        self.selection = primary.range;
        self.reversed = primary.reversed;
        self.caret_after_wrap = false;
        self.preferred_x = None;
        self.extra = edits.into_iter().map(|edit| edit.selection).collect();
        self.deduplicate_selections();
        self.moved(cx);
    }
}
