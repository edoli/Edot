//! Vault session: tabs, sidebar and explicitly expanded tree items.
use crate::{model::Notebook, storage::replace_file};
use std::{collections::HashSet, io, path::Path};

pub const DEFAULT_SIDEBAR_WIDTH: i32 = 260;
pub const MIN_SIDEBAR_WIDTH: i32 = 150;
pub const MAX_SIDEBAR_WIDTH: i32 = 600;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub tabs: Vec<u64>,
    pub active: Option<u64>,
    pub sidebar_visible: bool,
    pub sidebar_width: i32,
    pub expanded: HashSet<u64>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            tabs: vec![],
            active: None,
            sidebar_visible: true,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            expanded: HashSet::new(),
        }
    }
}

impl Session {
    pub fn load(dir: &Path, book: &Notebook) -> Self {
        std::fs::read_to_string(dir.join("session.tsv"))
            .ok()
            .and_then(|text| Self::parse(&text, book))
            .unwrap_or_default()
    }

    fn parse(text: &str, book: &Notebook) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next()? != "EDOTSESSION1" {
            return None;
        }
        let fields: Vec<_> = lines.next()?.split('\t').collect();
        if fields.len() != 3 || !matches!(fields[0], "0" | "1") {
            return None;
        }
        let mut session = Self {
            sidebar_visible: fields[0] == "1",
            sidebar_width: fields[1]
                .parse::<i32>()
                .ok()?
                .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH),
            active: fields[2].parse::<u64>().ok().filter(|id| *id != 0),
            tabs: vec![],
            expanded: HashSet::new(),
        };
        let valid: HashSet<_> = book.notes.iter().map(|note| note.id).collect();
        let folders = Self::folders(book);
        let mut seen = HashSet::new();
        for line in lines {
            if let Some(id) = line
                .strip_prefix("expanded\t")
                .and_then(|id| id.parse::<u64>().ok())
            {
                if folders.contains(&id) {
                    session.expanded.insert(id);
                }
                continue;
            }
            if let Ok(id) = line.parse::<u64>()
                && valid.contains(&id)
                && seen.insert(id)
            {
                session.tabs.push(id);
            }
        }
        if session.active.is_none_or(|id| !session.tabs.contains(&id)) {
            session.active = session.tabs.last().copied();
        }
        Some(session)
    }

    pub fn folders(book: &Notebook) -> HashSet<u64> {
        book.notes
            .iter()
            .filter_map(|note| (note.parent != 0).then_some(note.parent))
            .collect()
    }

    pub fn collapsed(&self, book: &Notebook) -> HashSet<u64> {
        Self::folders(book)
            .difference(&self.expanded)
            .copied()
            .collect()
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        let mut text = format!(
            "EDOTSESSION1\n{}\t{}\t{}\n",
            self.sidebar_visible as u8,
            self.sidebar_width,
            self.active.unwrap_or(0)
        );
        for id in &self.tabs {
            text.push_str(&format!("{id}\n"));
        }
        let mut expanded: Vec<_> = self.expanded.iter().copied().collect();
        expanded.sort_unstable();
        for id in expanded {
            text.push_str(&format!("expanded\t{id}\n"));
        }
        replace_file(&dir.join("session.tsv"), text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_session_filters_missing_duplicates_and_restores_order() {
        let mut book = Notebook {
            next_id: 1,
            ..Default::default()
        };
        book.add(0, "one".into());
        book.add(0, "two".into());
        let session = Session::parse("EDOTSESSION1\n0\t999\t55\n2\n99\n1\n2\n", &book).unwrap();
        assert_eq!(session.tabs, [2, 1]);
        assert_eq!(session.active, Some(1));
        assert_eq!(session.sidebar_width, 600);
        assert!(!session.sidebar_visible);
        assert!(Session::parse("EDOTSESSION1\nx\t260\t1", &book).is_none());
    }

    #[test]
    fn folders_default_to_collapsed_and_only_saved_expansions_restore() {
        let mut book = Notebook {
            next_id: 1,
            ..Default::default()
        };
        let root = book.add(0, "root".into());
        let child = book.add(root, "child".into());
        let nested = book.add(child, "nested".into());
        let legacy = Session::parse("EDOTSESSION1\n1\t260\t3\n3\n", &book).unwrap();
        assert_eq!(legacy.active, Some(nested));
        assert_eq!(legacy.collapsed(&book), HashSet::from([root, child]));
        let dir = std::env::temp_dir().join(format!(
            "edot-session-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let session = Session {
            expanded: HashSet::from([root, nested, 999]),
            ..legacy
        };
        session.save(&dir).unwrap();
        let restored = Session::load(&dir, &book);
        assert_eq!(restored.expanded, HashSet::from([root]));
        assert_eq!(restored.collapsed(&book), HashSet::from([child]));
        let new_root = book.add(0, "new".into());
        book.add(new_root, "new child".into());
        assert_eq!(
            Session::load(&dir, &book).collapsed(&book),
            HashSet::from([child, new_root])
        );
    }
}
