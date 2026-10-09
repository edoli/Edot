use super::*;

/// Global-search results and the work that can replace or incrementally update them.
pub(super) struct SearchResults {
    pub(super) groups: Vec<ResultRow>,
    pub(super) visible: Vec<VisibleRow>,
    pub(super) collapsed: HashSet<NoteId>,
    pub(super) cursor: Option<usize>,
    pub(super) generation: u64,
    pub(super) scanning: bool,
    pub(super) pending_navigation: Option<bool>,
    pub(super) scan_task: Option<Task<()>>,
    pub(super) note_tasks: HashMap<NoteId, Task<()>>,
    pub(super) pending_notes: HashSet<NoteId>,
    pub(super) errors: usize,
    pub(super) scroll: UniformListScrollHandle,
}
impl Default for SearchResults {
    fn default() -> Self {
        Self {
            groups: vec![],
            visible: vec![],
            collapsed: HashSet::new(),
            cursor: None,
            generation: 0,
            scanning: false,
            pending_navigation: None,
            scan_task: None,
            note_tasks: HashMap::new(),
            pending_notes: HashSet::new(),
            errors: 0,
            scroll: UniformListScrollHandle::new(),
        }
    }
}

pub(super) struct SearchPanel {
    pub(super) query: Entity<InputState>,
    pub(super) replacement: Entity<InputState>,
    pub(super) case: bool,
    pub(super) regex: bool,
    pub(super) replace: bool,
    pub(super) message: String,
    pub(super) cache: Cache,
    pub(super) last: Option<MatchKey>,
}
impl SearchPanel {
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            query: cx.new(|cx| InputState::new(window, cx).placeholder("찾을 내용")),
            replacement: cx.new(|cx| InputState::new(window, cx).placeholder("바꿀 내용")),
            case: false,
            regex: false,
            replace: false,
            message: "검색어를 입력하세요".into(),
            cache: Cache::default(),
            last: None,
        }
    }
    pub(super) fn pattern(&mut self, cx: &App) -> Result<Option<Pattern>, String> {
        let query = self.query.read(cx).value();
        if query.is_empty() {
            return Ok(None);
        }
        self.cache.get(&query, self.case, self.regex).map(Some)
    }
}

impl Edot {
    pub(super) fn show_find(
        &mut self,
        global: bool,
        replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !global && self.active_editor().is_none() {
            return;
        }
        self.settings_visible = false;
        if global {
            self.search_mode = true;
            self.session.sidebar_visible = true;
            self.find_visible = false;
        } else {
            self.find_visible = true;
        }
        let selected = self
            .active_editor()
            .map(|e| e.read(cx).selected_value().to_string())
            .unwrap_or_default();
        let panel = if global {
            &mut self.global
        } else {
            &mut self.local
        };
        panel.replace = replace;
        panel.query.update(cx, |state, cx| {
            if !selected.is_empty()
                && !selected.contains(['\r', '\n'])
                && selected.encode_utf16().count() <= 200
            {
                state.set_value(selected, window, cx);
            }
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        if global {
            self.queue_search(window, cx);
        } else {
            self.find_next(false, true, window, cx);
        }
        cx.notify();
    }
    pub(super) fn navigate(
        &mut self,
        global: bool,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if global {
            self.global_next(backwards, window, cx);
        } else {
            self.find_next(backwards, false, window, cx);
        }
    }
    pub(super) fn find_next(
        &mut self,
        backwards: bool,
        live: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let pattern = match self.local.pattern(cx) {
            Ok(Some(pattern)) => pattern,
            Ok(None) => {
                self.local.last = None;
                self.local.message = "검색어를 입력하세요".into();
                cx.notify();
                return;
            }
            Err(error) => {
                self.local.message = error;
                cx.notify();
                return;
            }
        };
        let text = editor.read(cx).value();
        let selected = editing::paragraph_range(&text, editor.read(cx).selected_range());
        let start = if backwards || live {
            selected.start
        } else {
            selected.end
        };
        let skip = !live && self.local.last.is_some_and(|key| key.range == selected);
        let mut ranges = vec![];
        pattern.visit_text(&text, |range| {
            ranges.push(range);
            true
        });
        let fits = |range: &&CharRange| {
            (if backwards {
                range.end <= start
            } else {
                range.start >= start
            }) && !(skip && range.start == start && range.end == start)
        };
        let found = if backwards {
            ranges.iter().rev().find(fits).or_else(|| ranges.last())
        } else {
            ranges.iter().find(fits).or_else(|| ranges.first())
        }
        .copied();
        self.local.last = found.map(|range| MatchKey {
            id: self.session.active.unwrap(),
            range,
        });
        self.local.message = if let Some(range) = found {
            let index = ranges.iter().position(|r| *r == range).unwrap_or(0) + 1;
            editor.update(cx, |state, cx| {
                state.set_selected_range(editing::byte_range(&text, range), cx)
            });
            format!("{index} / {}곳", ranges.len())
        } else {
            "일치하는 텍스트가 없습니다".into()
        };
        cx.notify();
    }
    pub(super) fn replace_local(&mut self, all: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        if editor.read(cx).is_read_only() {
            self.local.message = "미리보기는 읽기 전용입니다".into();
            cx.notify();
            return;
        }
        let pattern = match self.local.pattern(cx) {
            Ok(Some(pattern)) => pattern,
            Ok(None) => return,
            Err(error) => {
                self.local.message = error;
                cx.notify();
                return;
            }
        };
        let text = editor.read(cx).value();
        let replacement = self.local.replacement.read(cx).value();
        let edits = pattern.edits_text(&text, &replacement);
        if all {
            if !edits.is_empty() {
                let after = crate::search::apply_edits(&text, &edits);
                editor.update(cx, |state, cx| state.replace_all(after, window, cx));
                self.changed(self.session.active.unwrap(), window, cx);
            }
            self.local.message = format!("{}곳 바꿨습니다", edits.len());
        } else {
            let selected = editing::paragraph_range(&text, editor.read(cx).selected_range());
            if let Some(edit) = edits.iter().find(|edit| edit.range == selected)
                && (selected.start != selected.end
                    || self.local.last.is_some_and(|key| key.range == selected))
            {
                editor.update(cx, |state, cx| {
                    state.replace(edit.replacement.clone(), window, cx)
                });
                self.changed(self.session.active.unwrap(), window, cx);
            }
            self.find_next(false, false, window, cx);
        }
        cx.notify();
    }
}

mod replace;
mod results;
mod scan;
