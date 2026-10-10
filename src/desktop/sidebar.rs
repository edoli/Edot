//! Sidebar motion stays separate from the persisted visibility and width.
use gpui::{SpringConfig, SpringState};
use std::time::Instant;

const SPRING: SpringConfig = SpringConfig::new(1600., 80., 1.);
const EPSILON: f32 = 0.002;

pub(super) struct SidebarMotion {
    state: SpringState,
    visible: bool,
    last_frame: Instant,
}

impl SidebarMotion {
    pub(super) fn new(visible: bool, now: Instant) -> Self {
        Self {
            state: SpringState {
                position: if visible { 1. } else { 0. },
                velocity: 0.,
            },
            visible,
            last_frame: now,
        }
    }

    pub(super) fn tick(&mut self, visible: bool, now: Instant, reduced: bool) -> (f32, bool) {
        // Idle time must not count toward a newly requested transition. Reversals
        // keep the current position and velocity instead of restarting an animation.
        if visible != self.visible {
            self.visible = visible;
            self.last_frame = now;
        }
        let target = if visible { 1. } else { 0. };
        self.state = SPRING.step(
            self.state,
            target,
            now.saturating_duration_since(self.last_frame).as_secs_f32(),
        );
        self.last_frame = now;
        let done = reduced || SPRING.is_settled(self.state, target, EPSILON);
        if done {
            self.state = SpringState {
                position: target,
                velocity: 0.,
            };
        }
        (self.state.position.clamp(0., 1.), !done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn idle_close_reversal_and_reduced_motion_preserve_continuity_and_stop_frames() {
        let start = Instant::now();
        let mut motion = SidebarMotion::new(true, start);
        let idle = start + Duration::from_secs(60);
        assert_eq!(motion.tick(false, idle, false), (1., true));
        let halfway = idle + Duration::from_millis(50);
        let (position, moving) = motion.tick(false, halfway, false);
        assert!(moving && position > 0. && position < 1.);
        assert_eq!(motion.tick(true, halfway, false), (position, true));
        assert_eq!(
            motion.tick(true, halfway + Duration::from_secs(1), false),
            (1., false)
        );
        assert_eq!(
            motion.tick(false, halfway + Duration::from_secs(2), true),
            (0., false)
        );
        assert_eq!(
            motion.tick(false, halfway + Duration::from_secs(3), false),
            (0., false)
        );
    }
}
