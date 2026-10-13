use super::*;

#[test]
fn permanent_deletion_removes_only_the_confirmed_subtree_and_keeps_live_notes() {
    let (data, mut book, [root, child, other]) = fixture();
    book.trash_notes(&data.0, &[root]).unwrap();
    let entries = list(&data.0).unwrap();
    let selected = subtree(
        &entries,
        entries.iter().find(|e| e.note.id == root).unwrap(),
    );
    let keys: Vec<_> = selected.iter().map(Entry::key).collect();
    let tree = fs::read(data.0.join("tree.tsv")).unwrap();
    let notes = book.notes.clone();
    let result = permanently_delete(&data.0, &keys).unwrap();
    assert_eq!(result.count, 2);
    assert!(result.warning.is_none());
    assert!(list(&data.0).unwrap().is_empty());
    for entry in selected {
        assert!(!entry.body(&data.0).exists());
        assert!(!entry.metadata(&data.0).exists());
    }
    assert_eq!(book.notes, notes);
    assert_eq!(fs::read(data.0.join("tree.tsv")).unwrap(), tree);
    assert_eq!(
        fs::read_to_string(book.file(&data.0, other)).unwrap(),
        "보존"
    );
    assert!(!book.file(&data.0, child).exists());
    assert_eq!(permanently_delete(&data.0, &keys).unwrap().count, 0);
}

#[test]
fn empty_snapshot_keeps_new_archives_unknown_files_and_directories() {
    let (data, mut book, [root, _, other]) = fixture();
    book.trash_notes(&data.0, &[root]).unwrap();
    fs::write(data.0.join("trash/99-1.meta"), "old orphan metadata").unwrap();
    fs::write(data.0.join("trash/unknown.txt"), "preserve unknown").unwrap();
    fs::write(data.0.join("trash/01-2.txt"), "preserve noncanonical").unwrap();
    fs::create_dir(data.0.join("trash/98-1.txt")).unwrap();
    fs::write(data.0.join("trash/98-1.txt/nested.txt"), "preserve nested").unwrap();
    let keys = deletion_keys(&data.0).unwrap();
    assert_eq!(keys.len(), 3);
    // A new deletion after confirmation was opened must survive its snapshot.
    book.trash_notes(&data.0, &[other]).unwrap();
    let result = permanently_delete(&data.0, &keys).unwrap();
    assert_eq!(result.count, 3);
    assert!(result.warning.is_none());
    assert_eq!(list(&data.0).unwrap()[0].note.id, other);
    assert!(!data.0.join("trash/99-1.meta").exists());
    assert_eq!(
        fs::read_to_string(data.0.join("trash/unknown.txt")).unwrap(),
        "preserve unknown"
    );
    assert_eq!(
        fs::read_to_string(data.0.join("trash/01-2.txt")).unwrap(),
        "preserve noncanonical"
    );
    assert_eq!(
        fs::read_to_string(data.0.join("trash/98-1.txt/nested.txt")).unwrap(),
        "preserve nested"
    );
    permanently_delete(&data.0, &deletion_keys(&data.0).unwrap()).unwrap();
    assert!(list(&data.0).unwrap().is_empty());
    assert!(data.0.join("trash/unknown.txt").is_file());
}

#[test]
fn invalid_keys_and_replaced_directories_are_never_deleted() {
    let (data, mut book, [root, _, _]) = fixture();
    book.trash_notes(&data.0, &[root]).unwrap();
    let entry = list(&data.0).unwrap().pop().unwrap();
    for invalid in ["../notes/1", "1-2/../3", "0-1", "01-1", "1-01"] {
        assert!(permanently_delete(&data.0, &[entry.key(), invalid.into()]).is_err());
        assert!(entry.body(&data.0).is_file());
        assert!(entry.metadata(&data.0).is_file());
    }
    fs::remove_file(entry.body(&data.0)).unwrap();
    fs::create_dir(entry.body(&data.0)).unwrap();
    fs::write(entry.body(&data.0).join("keep.txt"), "preserve").unwrap();
    let result = permanently_delete(&data.0, &[entry.key()]).unwrap();
    assert_eq!(result.count, 0);
    assert!(result.warning.is_some());
    assert!(entry.metadata(&data.0).is_file());
    assert_eq!(
        fs::read_to_string(entry.body(&data.0).join("keep.txt")).unwrap(),
        "preserve"
    );
}

#[cfg(windows)]
#[test]
fn deletion_failures_preserve_metadata_and_orphans_can_be_retried() {
    use std::os::windows::fs::OpenOptionsExt;
    let (data, mut book, [root, child, _]) = fixture();
    book.trash_notes(&data.0, &[root]).unwrap();
    let entries = list(&data.0).unwrap();
    let root = entries.iter().find(|e| e.note.id == root).unwrap();
    let child = entries.iter().find(|e| e.note.id == child).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(root.body(&data.0))
        .unwrap();
    let result =
        permanently_delete(&data.0, &entries.iter().map(Entry::key).collect::<Vec<_>>()).unwrap();
    assert_eq!(result.count, 1);
    assert!(result.warning.is_some());
    assert!(root.body(&data.0).is_file());
    assert!(root.metadata(&data.0).is_file());
    assert!(!child.body(&data.0).exists());
    drop(lock);
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(root.metadata(&data.0))
        .unwrap();
    let result = permanently_delete(&data.0, &[root.key()]).unwrap();
    assert_eq!(result.count, 1);
    assert!(result.warning.is_some());
    assert!(!root.body(&data.0).exists());
    assert!(root.metadata(&data.0).is_file());
    drop(lock);
    let keys = deletion_keys(&data.0).unwrap();
    assert_eq!(keys, [root.key()]);
    let result = permanently_delete(&data.0, &keys).unwrap();
    assert_eq!(result.count, 1);
    assert!(result.warning.is_none());
    assert!(deletion_keys(&data.0).unwrap().is_empty());
}

struct Data(PathBuf);
impl Data {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "edot-trash-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("notes")).unwrap();
        Self(path)
    }
}
impl Drop for Data {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> (Data, Notebook, [NoteId; 3]) {
    let data = Data::new();
    let mut book = Notebook::default();
    let root = book.add(0, "한글 폴더 😀".into()).unwrap();
    let child = book.add(root, "하위 노트".into()).unwrap();
    let other = book.add(0, "기존 노트".into()).unwrap();
    for (id, body) in [
        (root, "폴더 본문\r\n"),
        (child, "하위 본문\r\n\r끝\n"),
        (other, "보존"),
    ] {
        fs::write(book.file(&data.0, id), body).unwrap();
    }
    book.save(&data.0).unwrap();
    (data, book, [root, child, other])
}

#[test]
fn archive_preview_and_subtree_restore_preserve_names_hierarchy_and_bytes() {
    let (data, mut book, [root, child, other]) = fixture();
    let original = book.notes.clone();
    book.trash_notes(&data.0, &[root]).unwrap();
    assert_eq!(book.notes.len(), 1);
    let entries = list(&data.0).unwrap();
    assert_eq!(entries.len(), 2);
    let entry = entries.iter().find(|e| e.note.id == root).unwrap();
    assert!(entry.has_metadata);
    assert_eq!(entry.note.title, "한글 폴더 😀");
    assert_eq!(
        preview(&data.0, entry).unwrap(),
        ("폴더 본문\r\n".into(), false)
    );
    assert_eq!(subtree(&entries, entry).len(), 2);
    let restored = restore(&mut book, &data.0, &entry.key()).unwrap();
    assert_eq!(restored.ids, [root, child]);
    assert!(restored.warning.is_none());
    for note in original {
        assert_eq!(book.get(note.id), Some(&note));
    }
    assert_eq!(book.get(child).unwrap().parent, root);
    assert_eq!(
        fs::read_to_string(book.file(&data.0, child)).unwrap(),
        "하위 본문\r\n\r끝\n"
    );
    assert_eq!(
        fs::read_to_string(book.file(&data.0, other)).unwrap(),
        "보존"
    );
    assert!(list(&data.0).unwrap().is_empty());
    assert_eq!(fs::read_dir(data.0.join("trash")).unwrap().count(), 0);
    assert_eq!(
        Notebook::reload(&data.0)
            .unwrap()
            .get(child)
            .unwrap()
            .parent,
        root
    );
}

#[test]
fn restore_remaps_collisions_without_overwriting_registered_or_orphan_bodies() {
    let (data, mut book, [root, child, _]) = fixture();
    book.trash_notes(&data.0, &[root]).unwrap();
    book.notes.push(Note {
        id: root,
        parent: 0,
        order: 0,
        title: "기존 충돌 노트".into(),
    });
    fs::write(book.file(&data.0, root), "existing root").unwrap();
    fs::write(book.file(&data.0, child), "unregistered external file").unwrap();
    book.save(&data.0).unwrap();
    let entry = list(&data.0)
        .unwrap()
        .into_iter()
        .find(|e| e.note.id == root)
        .unwrap();
    let restored = restore(&mut book, &data.0, &entry.key()).unwrap();
    let [new_root, new_child] = restored.ids.as_slice() else {
        panic!("whole subtree expected")
    };
    assert_ne!(*new_root, root);
    assert_ne!(*new_child, child);
    assert_eq!(book.get(*new_child).unwrap().parent, *new_root);
    assert_eq!(
        fs::read_to_string(book.file(&data.0, *new_root)).unwrap(),
        "폴더 본문\r\n"
    );
    assert_eq!(
        fs::read_to_string(book.file(&data.0, root)).unwrap(),
        "existing root"
    );
    assert_eq!(
        fs::read_to_string(book.file(&data.0, child)).unwrap(),
        "unregistered external file"
    );
    Notebook::reload(&data.0).unwrap();
}

#[test]
fn failed_restore_keeps_the_archive_model_and_index_and_allows_retry() {
    let (data, mut book, [root, child, _]) = fixture();
    book.trash_notes(&data.0, &[root]).unwrap();
    let entries = list(&data.0).unwrap();
    let root_entry = entries.iter().find(|e| e.note.id == root).unwrap();
    let model = book.notes.clone();
    let index = fs::read(data.0.join("tree.tsv")).unwrap();
    fs::create_dir(data.0.join("tree.tmp")).unwrap();
    assert!(restore(&mut book, &data.0, &root_entry.key()).is_err());
    assert_eq!(book.notes, model);
    assert_eq!(fs::read(data.0.join("tree.tsv")).unwrap(), index);
    assert!(!book.file(&data.0, root).exists());
    assert!(!book.file(&data.0, child).exists());
    for entry in &entries {
        assert!(entry.body(&data.0).is_file());
        assert!(entry.metadata(&data.0).is_file());
    }
    fs::remove_dir(data.0.join("tree.tmp")).unwrap();
    restore(&mut book, &data.0, &root_entry.key()).unwrap();
}

#[test]
fn old_or_damaged_metadata_is_viewable_and_missing_parents_restore_at_root() {
    let (data, mut book, [root, child, _]) = fixture();
    book.trash_notes(&data.0, &[child]).unwrap();
    let entry = list(&data.0).unwrap().pop().unwrap();
    restore(&mut book, &data.0, &entry.key()).unwrap();
    assert_eq!(book.get(child).unwrap().parent, root);
    book.trash_notes(&data.0, &[root]).unwrap();
    let mut entries = list(&data.0).unwrap();
    let child_entry = entries.iter().find(|e| e.note.id == child).unwrap();
    restore(&mut book, &data.0, &child_entry.key()).unwrap();
    assert_eq!(book.get(child).unwrap().parent, 0);
    let root_entry = entries.iter().find(|e| e.note.id == root).unwrap();
    fs::write(root_entry.metadata(&data.0), "broken metadata").unwrap();
    entries = list(&data.0).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].has_metadata);
    assert_eq!(preview(&data.0, &entries[0]).unwrap().0, "폴더 본문\r\n");
    fs::remove_file(entries[0].metadata(&data.0)).unwrap();
    let legacy = list(&data.0).unwrap().pop().unwrap();
    restore(&mut book, &data.0, &legacy.key()).unwrap();
    assert_eq!(book.get(root).unwrap().title, format!("삭제된 노트 {root}"));
    assert_eq!(book.get(root).unwrap().parent, 0);
    fs::write(data.0.join("trash/not-a-note.txt"), "preserve").unwrap();
    assert!(list(&data.0).unwrap().is_empty());
}

#[test]
fn preview_is_bounded_and_keeps_valid_utf8_without_changing_the_body() {
    let (data, mut book, [root, _, _]) = fixture();
    let body = "가".repeat((PREVIEW_LIMIT / 3 + 20) as usize);
    fs::write(book.file(&data.0, root), &body).unwrap();
    book.trash_notes(&data.0, &[root]).unwrap();
    let entry = list(&data.0)
        .unwrap()
        .into_iter()
        .find(|e| e.note.id == root)
        .unwrap();
    let (text, truncated) = preview(&data.0, &entry).unwrap();
    assert!(truncated);
    assert_eq!(text, "가".repeat((PREVIEW_LIMIT / 3) as usize));
    assert_eq!(fs::read_to_string(entry.body(&data.0)).unwrap(), body);
    fs::write(entry.body(&data.0), [0xff]).unwrap();
    assert_eq!(
        preview(&data.0, &entry).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}
