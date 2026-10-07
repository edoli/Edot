//! The native application's session format, independent of the UI backend.
use crate::{model::Notebook, storage::replace_file};
use std::{io, path::Path};

pub const DEFAULT_SIDEBAR_WIDTH: i32 = 260;
pub const MIN_SIDEBAR_WIDTH: i32 = 150;
pub const MAX_SIDEBAR_WIDTH: i32 = 600;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub tabs: Vec<u64>,
    pub active: Option<u64>,
    pub sidebar_visible: bool,
    pub sidebar_width: i32,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            tabs: vec![],
            active: None,
            sidebar_visible: true,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
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
        };
        for line in lines {
            if let Ok(id) = line.parse::<u64>()
                && book.get(id).is_some()
                && !session.tabs.contains(&id)
            {
                session.tabs.push(id);
            }
        }
        if session.active.is_none_or(|id| !session.tabs.contains(&id)) {
            session.active = session.tabs.last().copied();
        }
        Some(session)
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
}
