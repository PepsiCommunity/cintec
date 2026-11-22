//! Recovery policy is independent of GPUI and the audio backend.
use std::time::{Duration, Instant};
#[derive(Clone, Debug)]
pub struct Recovery {
    pub position: f64,
    pub playing: bool,
    pub loaded: bool,
    pub due: Instant,
    pub attempts: u32,
}
impl Recovery {
    pub fn capture(position: f64, playing: bool, loaded: bool) -> Self {
        Self {
            position: if position.is_finite() {
                position.max(0.)
            } else {
                0.
            },
            playing: playing && loaded,
            loaded,
            due: Instant::now(),
            attempts: 0,
        }
    }
    pub fn retry(&mut self) {
        self.attempts = self.attempts.saturating_add(1);
        self.due = Instant::now() + Duration::from_secs(1 << self.attempts.min(4));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paused_and_stopped_remain_distinct_after_sleep() {
        let paused = Recovery::capture(42.5, false, true);
        assert!(paused.loaded);
        assert!(!paused.playing);
        assert_eq!(paused.position, 42.5);
        let stopped = Recovery::capture(12., false, false);
        assert!(!stopped.loaded);
        assert!(!stopped.playing);
        let playing = Recovery::capture(95., true, true);
        assert!(playing.loaded && playing.playing);
        assert_eq!(playing.position, 95.);
    }
    #[test]
    fn retry_retains_snapshot_and_caps_backoff() {
        let mut r = Recovery::capture(123., true, true);
        for _ in 0..30 {
            r.retry();
        }
        assert_eq!(r.position, 123.);
        assert!(r.playing && r.loaded);
        let delay = r.due.saturating_duration_since(Instant::now());
        assert!(delay <= Duration::from_secs(16) && delay > Duration::from_secs(15));
    }
    #[test]
    fn invalid_position_cannot_poison_recovery() {
        assert_eq!(Recovery::capture(f64::NAN, true, false).position, 0.);
        assert!(!Recovery::capture(0., true, false).playing);
    }
}
