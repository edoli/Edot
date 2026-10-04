use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: u64,
    pub parent: u64,
    pub order: u32,
    pub title: String,
}

#[derive(Clone, Default)]
pub struct Notebook {
    pub notes: Vec<Note>,
    pub next_id: u64,
}

impl Notebook {
    pub fn load(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir.join("notes"))?;
        let path = dir.join("tree.tsv");
        let text = if path.exists() {
            fs::read_to_string(path)?
        } else {
            String::from("EDOT1\n")
        };
        let mut lines = text.lines();
        if lines.next() != Some("EDOT1") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported tree index",
            ));
        }
        let mut notes = Vec::new();
        for line in lines {
            let mut parts = line.split('\t');
            let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid tree index");
            let id = parts
                .next()
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?;
            let parent = parts
                .next()
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?;
            let order = parts
                .next()
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?;
            let title = String::from_utf8(decode_hex(parts.next().ok_or_else(invalid)?)?)
                .map_err(|_| invalid())?;
            if parts.next().is_some() || id == 0 || notes.iter().any(|n: &Note| n.id == id) {
                return Err(invalid());
            }
            notes.push(Note {
                id,
                parent,
                order,
                title,
            });
        }
        for note in &notes {
            if note.parent != 0 && !notes.iter().any(|n| n.id == note.parent) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Missing parent note",
                ));
            }
        }
        // If a previous write stopped after creating a text file, keep that file visible.
        for entry in fs::read_dir(dir.join("notes"))? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let filename = entry.file_name();
            let name = filename.to_string_lossy();
            let Some(id) = name
                .strip_suffix(".txt")
                .and_then(|s| s.parse::<u64>().ok())
            else {
                continue;
            };
            if id == 0 || notes.iter().any(|n| n.id == id) {
                continue;
            }
            let order = notes.iter().filter(|n| n.parent == 0).count() as u32;
            notes.push(Note {
                id,
                parent: 0,
                order,
                title: format!("복구된 메모 {id}"),
            });
        }
        let next_id = notes
            .iter()
            .map(|n| n.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let book = Self { notes, next_id };
        if book.notes.iter().any(|n| book.is_descendant(n.id, n.id)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Tree contains a cycle",
            ));
        }
        Ok(book)
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        let mut text = String::from("EDOT1\n");
        for n in &self.notes {
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                n.id,
                n.parent,
                n.order,
                encode_hex(n.title.as_bytes())
            ));
        }
        replace_file(&dir.join("tree.tsv"), text.as_bytes())
    }

    pub fn file(&self, dir: &Path, id: u64) -> PathBuf {
        dir.join("notes").join(format!("{id}.txt"))
    }

    pub fn add(&mut self, parent: u64, title: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let order = self.notes.iter().filter(|n| n.parent == parent).count() as u32;
        self.notes.push(Note {
            id,
            parent,
            order,
            title,
        });
        id
    }

    pub fn get(&self, id: u64) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    pub fn children(&self, parent: u64) -> Vec<u64> {
        let mut children: Vec<_> = self.notes.iter().filter(|n| n.parent == parent).collect();
        children.sort_by_key(|n| (n.order, n.id));
        children.into_iter().map(|n| n.id).collect()
    }

    pub fn is_descendant(&self, ancestor: u64, candidate: u64) -> bool {
        let mut current = self.get(candidate).map(|n| n.parent).unwrap_or(0);
        for _ in 0..self.notes.len() {
            if current == 0 {
                return false;
            }
            if current == ancestor {
                return true;
            }
            current = self.get(current).map(|n| n.parent).unwrap_or(0);
        }
        true
    }

    pub fn move_note(&mut self, id: u64, parent: u64, before: Option<u64>) -> bool {
        if id == parent
            || (parent != 0 && self.get(parent).is_none())
            || self.is_descendant(id, parent)
        {
            return false;
        }
        if let Some(before_id) = before
            && (before_id == id || self.get(before_id).is_none_or(|n| n.parent != parent))
        {
            return false;
        }
        let old_parent = match self.get(id) {
            Some(n) => n.parent,
            None => return false,
        };
        let mut old_siblings = self.children(old_parent);
        old_siblings.retain(|&x| x != id);
        let mut new_siblings = if old_parent == parent {
            old_siblings.clone()
        } else {
            self.children(parent)
        };
        let position = before
            .and_then(|b| new_siblings.iter().position(|&x| x == b))
            .unwrap_or(new_siblings.len());
        new_siblings.insert(position, id);
        if let Some(note) = self.get_mut(id) {
            note.parent = parent;
        }
        self.set_orders(old_parent, &old_siblings);
        self.set_orders(parent, &new_siblings);
        true
    }

    fn set_orders(&mut self, parent: u64, siblings: &[u64]) {
        for (order, &id) in siblings.iter().enumerate() {
            if let Some(n) = self.get_mut(id) {
                n.parent = parent;
                n.order = order as u32;
            }
        }
    }

    pub fn subtree(&self, id: u64) -> Vec<u64> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            out.extend(self.children(out[i]));
            i += 1;
        }
        out
    }

    pub fn remove(&mut self, id: u64) -> Vec<u64> {
        let ids = self.subtree(id);
        self.notes.retain(|n| !ids.contains(&n.id));
        ids
    }
}

pub fn replace_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 15) as usize] as char);
    }
    out
}

fn decode_hex(text: &str) -> io::Result<Vec<u8>> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "Invalid title encoding");
    if !text.len().is_multiple_of(2) {
        return Err(bad());
    }
    let mut out = Vec::new();
    for pair in text.as_bytes().chunks_exact(2) {
        let digit = |c: u8| (c as char).to_digit(16).map(|v| v as u8).ok_or_else(bad);
        out.push((digit(pair[0])? << 4) | digit(pair[1])?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moving_and_persisting_nested_notes() {
        let dir = std::env::temp_dir().join(format!("edot-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut book = Notebook::load(&dir).unwrap();
        let a = book.add(0, "가나다".into());
        let b = book.add(0, "B".into());
        let c = book.add(a, "C".into());
        assert!(!book.move_note(a, c, None));
        assert!(book.move_note(b, a, Some(c)));
        assert_eq!(book.children(a), vec![b, c]);
        book.save(&dir).unwrap();
        let loaded = Notebook::load(&dir).unwrap();
        assert_eq!(loaded.get(a).unwrap().title, "가나다");
        assert_eq!(loaded.children(a), vec![b, c]);
        let mut removed = loaded.clone();
        assert_eq!(removed.remove(a), vec![a, b, c]);
        assert!(removed.notes.is_empty());
        let file = loaded.file(&dir, a);
        replace_file(&file, "첫 내용".as_bytes()).unwrap();
        replace_file(&file, "수정된 내용".as_bytes()).unwrap();
        assert_eq!(fs::read_to_string(file).unwrap(), "수정된 내용");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recovers_an_unindexed_text_file_without_reusing_its_id() {
        let dir = std::env::temp_dir().join(format!("edot-orphan-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("notes")).unwrap();
        fs::write(dir.join("notes/7.txt"), "살아있는 내용").unwrap();
        let mut book = Notebook::load(&dir).unwrap();
        assert_eq!(book.get(7).unwrap().title, "복구된 메모 7");
        assert_eq!(book.add(0, "새 메모".into()), 8);
        let _ = fs::remove_dir_all(dir);
    }
}
