//! Opt-in startup timings. No note contents are recorded.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Profile {
    path: PathBuf,
    started: Instant,
    timings: BTreeMap<&'static str, (usize, Duration)>,
}
thread_local! {
    static PROFILE: RefCell<Option<Profile>> = const { RefCell::new(None) };
}
pub fn start() {
    if let Some(path) = std::env::var_os("EDOT_STARTUP_PROFILE") {
        PROFILE.with(|p| {
            *p.borrow_mut() = Some(Profile {
                path: path.into(),
                started: Instant::now(),
                timings: BTreeMap::new(),
            });
        });
    }
}
pub struct Span {
    label: &'static str,
    started: Option<Instant>,
}
pub fn span(label: &'static str) -> Span {
    Span {
        label,
        started: PROFILE.with(|p| p.borrow().as_ref().map(|_| Instant::now())),
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            PROFILE.with(|p| {
                if let Some(profile) = p.borrow_mut().as_mut() {
                    let timing = profile.timings.entry(self.label).or_default();
                    timing.0 += 1;
                    timing.1 += started.elapsed();
                }
            });
        }
    }
}
pub fn finish() -> bool {
    PROFILE.with(|p| {
        let Some(profile) = p.borrow_mut().take() else {
            return false;
        };
        let mut report = format!(
            "startup_ms\t{:.3}\n",
            profile.started.elapsed().as_secs_f64() * 1000.0
        );
        for (label, (count, elapsed)) in profile.timings {
            report.push_str(&format!(
                "{label}\t{count}\t{:.3}\n",
                elapsed.as_secs_f64() * 1000.0
            ));
        }
        let _ = std::fs::write(profile.path, report);
        std::env::var_os("EDOT_STARTUP_EXIT").is_some()
    })
}
