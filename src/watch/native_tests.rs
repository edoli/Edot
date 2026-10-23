//! The same contract runs against ReadDirectoryChangesW, inotify and FSEvents.
use super::{Changes, Events, Watcher};
use std::{
    fs, io,
    path::PathBuf,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "edot-watch-contract-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let fixture = Self(root);
        fs::create_dir_all(fixture.space().join("notes")).unwrap();
        fs::create_dir(fixture.space().join(".edot")).unwrap();
        fixture
    }
    fn space(&self) -> PathBuf {
        self.0.join("space")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn wait(events: &Events, predicate: impl Fn(&Changes) -> bool) -> io::Result<Changes> {
    let start = Instant::now();
    let mut collected = Changes::default();
    loop {
        if let Some(result) = events.take() {
            collected.merge(result?);
        }
        if predicate(&collected) {
            return Ok(collected);
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "missing event: {collected:?}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn tracks_write_atomic_replace_rename_delete_and_ignores_unrelated_files() {
    let fixture = Fixture::new();
    let dir = fixture.space();
    let (watcher, events) = Watcher::new(&dir).unwrap();
    wait(&events, |c| c.rescan).unwrap();
    fs::write(dir.join("notes/7.txt"), "first").unwrap();
    wait(&events, |c| c.notes.contains(&7) || c.rescan).unwrap();
    fs::write(dir.join("notes/7.txt"), "second").unwrap();
    wait(&events, |c| c.notes.contains(&7) || c.rescan).unwrap();
    fs::write(dir.join("notes/7.tmp"), "atomic").unwrap();
    fs::rename(dir.join("notes/7.tmp"), dir.join("notes/7.txt")).unwrap();
    wait(&events, |c| c.notes.contains(&7) || c.rescan).unwrap();
    fs::rename(dir.join("notes/7.txt"), dir.join("notes/8.txt")).unwrap();
    wait(&events, |c| {
        c.rescan || c.notes.contains(&7) && c.notes.contains(&8)
    })
    .unwrap();
    fs::remove_file(dir.join("notes/8.txt")).unwrap();
    wait(&events, |c| c.notes.contains(&8) || c.rescan).unwrap();
    fs::write(dir.join(".edot/tree.tmp"), "index").unwrap();
    fs::rename(dir.join(".edot/tree.tmp"), dir.join(".edot/tree.tsv")).unwrap();
    wait(&events, |c| c.tree || c.rescan).unwrap();
    // Drain coalesced macOS events before checking the deliberately ignored paths.
    thread::sleep(Duration::from_millis(300));
    while let Some(result) = events.take() {
        result.unwrap();
    }
    for name in [
        "notes/9.tmp",
        ".edot/session.bin",
        ".edot/space.ini",
        ".edot/.space.lock",
        "tree.tsv",
    ] {
        fs::write(dir.join(name), "ignored").unwrap();
    }
    #[cfg(unix)]
    for name in ["notes/9.TXT", ".edot/tree.TSV", "Notes/9.txt"] {
        fs::create_dir_all(dir.join(name).parent().unwrap()).unwrap();
        fs::write(dir.join(name), "wrong case").unwrap();
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(400) {
        if let Some(result) = events.take() {
            assert!(result.unwrap().is_empty());
        }
        thread::sleep(Duration::from_millis(5));
    }
    let start = Instant::now();
    drop(watcher);
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "idle cancellation must wake the watcher"
    );
    assert!(events.0.lock().unwrap().closed);
}

#[test]
fn replaced_directories_can_be_rewatched_and_missing_children_are_attached() {
    let fixture = Fixture::new();
    let dir = fixture.space();
    for child in ["notes", ".edot"] {
        let (watcher, events) = Watcher::new(&dir).unwrap();
        wait(&events, |c| c.rescan).unwrap();
        fs::rename(dir.join(child), fixture.0.join(format!("moved-{child}"))).unwrap();
        // Linux invalidates watches on the moved inode; Windows/macOS can rescan in place.
        let _ = wait(&events, |c| c.rescan);
        drop(watcher);
        // Startup with absent children is valid: root changes trigger reconciliation/restart.
        let (watcher, events) = Watcher::new(&dir).unwrap();
        wait(&events, |c| c.rescan).unwrap();
        fs::create_dir(dir.join(child)).unwrap();
        let _ = wait(&events, |c| c.rescan);
        drop(watcher);
        let (watcher, events) = Watcher::new(&dir).unwrap();
        wait(&events, |c| c.rescan).unwrap();
        if child == "notes" {
            fs::write(dir.join("notes/19.txt"), "new directory").unwrap();
            wait(&events, |c| c.notes.contains(&19) || c.rescan).unwrap();
        } else {
            fs::write(dir.join(".edot/tree.tsv"), "new index").unwrap();
            wait(&events, |c| c.tree || c.rescan).unwrap();
        }
        drop(watcher);
    }
}

#[cfg(unix)]
#[test]
fn moved_space_invalidates_the_receiver_and_releases_all_resources() {
    let fixture = Fixture::new();
    let (watcher, events) = Watcher::new(&fixture.space()).unwrap();
    wait(&events, |c| c.rescan).unwrap();
    fs::rename(fixture.space(), fixture.0.join("moved-space")).unwrap();
    assert!(wait(&events, |_| false).is_err());
    drop(watcher);
    assert!(events.0.lock().unwrap().closed);
    assert!(Watcher::new(&fixture.space()).is_err());
}

#[cfg(unix)]
#[test]
fn stopping_a_busy_watcher_does_not_wait_for_writes_to_finish() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let fixture = Fixture::new();
    let (watcher, events) = Watcher::new(&fixture.space()).unwrap();
    wait(&events, |c| c.rescan).unwrap();
    let running = Arc::new(AtomicBool::new(true));
    let writer_running = running.clone();
    let path = fixture.space().join("notes/23.txt");
    let writer = thread::spawn(move || {
        while writer_running.load(Ordering::Relaxed) {
            fs::write(&path, "ongoing edits").unwrap();
        }
    });
    wait(&events, |c| c.rescan || c.notes.contains(&23)).unwrap();
    let start = Instant::now();
    drop(watcher);
    let elapsed = start.elapsed();
    running.store(false, Ordering::Relaxed);
    writer.join().unwrap();
    assert!(elapsed < Duration::from_secs(2));
}
