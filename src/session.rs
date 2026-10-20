//! Space session: tabs, sidebar and explicitly expanded tree items.
use crate::model::NoteId;
use crate::{model::Notebook, storage::replace_file};
use std::{collections::HashSet, io, path::Path};

pub const DEFAULT_SIDEBAR_WIDTH: i32 = 260;
pub const MIN_SIDEBAR_WIDTH: i32 = 150;
pub const MAX_SIDEBAR_WIDTH: i32 = 600;
pub const FILE_NAME: &str = "session.bin";
const MAGIC: &[u8; 4] = b"EDSN";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 24;

fn encoded_len(tabs: usize, expanded: usize) -> Option<usize> {
    tabs.checked_add(expanded)?
        .checked_mul(8)?
        .checked_add(HEADER_LEN)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub tabs: Vec<NoteId>,
    pub active: Option<NoteId>,
    pub sidebar_visible: bool,
    pub sidebar_width: i32,
    pub expanded: HashSet<NoteId>,
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
        std::fs::read(dir.join(FILE_NAME))
            .ok()
            .and_then(|bytes| Self::parse(&bytes, book))
            .unwrap_or_default()
    }

    fn parse(bytes: &[u8], book: &Notebook) -> Option<Self> {
        if bytes.len() < HEADER_LEN
            || &bytes[..4] != MAGIC
            || bytes[4] != VERSION
            || bytes[5] & !1 != 0
        {
            return None;
        }
        let tab_count = usize::try_from(u32::from_le_bytes(bytes[16..20].try_into().ok()?)).ok()?;
        let expanded_count =
            usize::try_from(u32::from_le_bytes(bytes[20..24].try_into().ok()?)).ok()?;
        // Validate lengths before allocating collections or slicing the ID arrays.
        if bytes.len() != encoded_len(tab_count, expanded_count)? {
            return None;
        }
        let active = NoteId::from_le_bytes(bytes[8..16].try_into().ok()?);
        let mut session = Self {
            sidebar_visible: bytes[5] & 1 != 0,
            sidebar_width: i32::from(u16::from_le_bytes(bytes[6..8].try_into().ok()?)),
            active: (active != 0).then_some(active),
            tabs: vec![],
            expanded: HashSet::new(),
        };
        let tabs_end = HEADER_LEN + tab_count * 8;
        for chunk in bytes[HEADER_LEN..tabs_end].as_chunks::<8>().0 {
            session.tabs.push(NoteId::from_le_bytes(*chunk));
        }
        for chunk in bytes[tabs_end..].as_chunks::<8>().0 {
            session.expanded.insert(NoteId::from_le_bytes(*chunk));
        }
        Some(session.restore(book))
    }

    fn restore(mut self, book: &Notebook) -> Self {
        let valid: HashSet<_> = book
            .notes
            .iter()
            .filter(|note| !note.is_folder())
            .map(|note| note.id)
            .collect();
        let folders = Self::folders(book);
        let mut seen = HashSet::new();
        self.tabs
            .retain(|id| valid.contains(id) && seen.insert(*id));
        self.expanded.retain(|id| folders.contains(id));
        self.sidebar_width = self
            .sidebar_width
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        if self.active.is_none_or(|id| !self.tabs.contains(&id)) {
            self.active = self.tabs.last().copied();
        }
        self
    }

    pub fn folders(book: &Notebook) -> HashSet<NoteId> {
        book.notes
            .iter()
            .filter_map(|note| note.is_folder().then_some(note.id))
            .collect()
    }

    pub fn collapsed(&self, book: &Notebook) -> HashSet<NoteId> {
        Self::folders(book)
            .difference(&self.expanded)
            .copied()
            .collect()
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        replace_file(&dir.join(FILE_NAME), &self.encode()?)
    }

    fn encode(&self) -> io::Result<Vec<u8>> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "세션 항목이 너무 많습니다.");
        let tab_count = u32::try_from(self.tabs.len()).map_err(|_| invalid())?;
        let expanded_count = u32::try_from(self.expanded.len()).map_err(|_| invalid())?;
        let len = encoded_len(self.tabs.len(), self.expanded.len()).ok_or_else(invalid)?;
        let width = self
            .sidebar_width
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH) as u16;
        let mut bytes = Vec::with_capacity(len);
        bytes.extend_from_slice(MAGIC);
        bytes.push(VERSION);
        bytes.push(u8::from(self.sidebar_visible));
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&self.active.unwrap_or(0).to_le_bytes());
        bytes.extend_from_slice(&tab_count.to_le_bytes());
        bytes.extend_from_slice(&expanded_count.to_le_bytes());
        for id in &self.tabs {
            bytes.extend_from_slice(&id.to_le_bytes());
        }
        let mut expanded: Vec<_> = self.expanded.iter().copied().collect();
        expanded.sort_unstable();
        for id in expanded {
            bytes.extend_from_slice(&id.to_le_bytes());
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "edot-session-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    fn book() -> Notebook {
        Notebook {
            notes: [
                (u64::MAX, 7),
                (0x0102030405060708, 7),
                (1 << 63, 8),
                (7, 0),
                (8, 7),
            ]
            .into_iter()
            .map(|(id, parent)| crate::model::Note {
                id,
                parent,
                order: 0,
                title: String::new(),
                kind: if matches!(id, 7 | 8) {
                    crate::model::NoteKind::Folder
                } else {
                    crate::model::NoteKind::Note
                },
            })
            .collect(),
        }
    }

    #[test]
    fn binary_layout_preserves_full_width_ids_and_order() {
        let session = Session {
            tabs: vec![u64::MAX, 0x0102030405060708],
            active: Some(0x0102030405060708),
            sidebar_visible: false,
            sidebar_width: 300,
            expanded: HashSet::from([7, 8]),
        };
        // A wire-format fixture, independent of the encoder's field-writing code.
        let expected = [
            b'E', b'D', b'S', b'N', 1, 0, 0x2c, 1, 8, 7, 6, 5, 4, 3, 2, 1, 2, 0, 0, 0, 2, 0, 0, 0,
            255, 255, 255, 255, 255, 255, 255, 255, 8, 7, 6, 5, 4, 3, 2, 1, 7, 0, 0, 0, 0, 0, 0, 0,
            8, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(session.encode().unwrap(), expected);
        assert_eq!(Session::parse(&expected, &book()), Some(session));
        let empty = Session::default().encode().unwrap();
        assert_eq!(empty.len(), 24);
        assert_eq!(Session::parse(&empty, &book()), Some(Session::default()));
    }

    #[test]
    fn binary_rejects_truncation_unknown_headers_and_inconsistent_counts() {
        let bytes = Session {
            tabs: vec![u64::MAX],
            active: Some(u64::MAX),
            expanded: HashSet::from([7]),
            ..Default::default()
        }
        .encode()
        .unwrap();
        for end in 0..bytes.len() {
            assert!(
                Session::parse(&bytes[..end], &book()).is_none(),
                "length {end}"
            );
        }
        for (offset, value) in [(0, b'X'), (4, 2), (5, 2), (16, 0), (20, 0)] {
            let mut invalid = bytes.clone();
            invalid[offset] = value;
            assert!(
                Session::parse(&invalid, &book()).is_none(),
                "offset {offset}"
            );
        }
        for offset in [16, 20] {
            let mut invalid = bytes.clone();
            invalid[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(Session::parse(&invalid, &book()).is_none());
        }
        let mut invalid = bytes;
        invalid.push(0);
        assert!(Session::parse(&invalid, &book()).is_none());
        assert!(encoded_len(usize::MAX, 1).is_none());
        assert!(encoded_len(usize::MAX / 8, 0).is_none());
    }

    #[test]
    fn binary_filters_stale_ids_and_clamps_width_on_restore() {
        let session = Session {
            tabs: vec![0, 7, 0x0102030405060708, u64::MAX, 0x0102030405060708],
            active: Some(9),
            expanded: HashSet::from([0, 7, 8, 9, u64::MAX, 0x0102030405060708, 1 << 63]),
            ..Default::default()
        };
        for (width, expected_width) in [(0u16, MIN_SIDEBAR_WIDTH), (u16::MAX, MAX_SIDEBAR_WIDTH)] {
            let mut bytes = session.encode().unwrap();
            bytes[6..8].copy_from_slice(&width.to_le_bytes());
            let restored = Session::parse(&bytes, &book()).unwrap();
            assert_eq!(restored.tabs, [0x0102030405060708, u64::MAX]);
            assert_eq!(restored.active, Some(u64::MAX));
            assert_eq!(restored.expanded, HashSet::from([7, 8]));
            assert_eq!(restored.sidebar_width, expected_width);
        }
    }

    #[test]
    fn session_load_defaults_on_missing_or_invalid_binary_and_round_trips() {
        let dir = test_dir();
        assert_eq!(Session::load(&dir, &book()), Session::default());
        let session = Session {
            tabs: vec![u64::MAX],
            active: Some(u64::MAX),
            expanded: HashSet::from([7]),
            ..Default::default()
        };
        session.save(&dir).unwrap();
        assert_eq!(Session::load(&dir, &book()), session);
        std::fs::write(dir.join(FILE_NAME), b"damaged").unwrap();
        assert_eq!(Session::load(&dir, &book()), Session::default());
        std::fs::remove_file(dir.join(FILE_NAME)).unwrap();
        std::fs::create_dir(dir.join(FILE_NAME)).unwrap();
        assert_eq!(Session::load(&dir, &book()), Session::default());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn folders_default_to_collapsed_and_only_saved_expansions_restore() {
        let mut book = Notebook::default();
        let root = book.add_folder(0, "root".into()).unwrap();
        let child = book.add_folder(root, "child".into()).unwrap();
        let nested = book.add(child, "nested".into()).unwrap();
        let session = Session {
            tabs: vec![nested],
            active: Some(nested),
            ..Default::default()
        };
        assert_eq!(session.active, Some(nested));
        assert_eq!(session.collapsed(&book), HashSet::from([root, child]));
        let dir = test_dir();
        let session = Session {
            expanded: HashSet::from([root, nested, 999]),
            ..session
        };
        session.save(&dir).unwrap();
        let restored = Session::load(&dir, &book);
        assert_eq!(restored.expanded, HashSet::from([root]));
        assert_eq!(restored.collapsed(&book), HashSet::from([child]));
        let new_root = book.add_folder(0, "new".into()).unwrap();
        book.add(new_root, "new child".into()).unwrap();
        assert_eq!(
            Session::load(&dir, &book).collapsed(&book),
            HashSet::from([child, new_root])
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
