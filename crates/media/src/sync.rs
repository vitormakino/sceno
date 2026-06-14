//! Playback-position extrapolation between player samples.

use std::time::Instant;

/// Reference point that lets us extrapolate the current playback position
/// between sync samples from the player.
#[derive(Debug, Clone)]
pub struct TimelineSync {
    pub video_time: f64,
    pub captured_at: Instant,
    pub paused: bool,
    pub playback_rate: f64,
}

impl TimelineSync {
    /// Current playback position: the captured time when paused, otherwise
    /// extrapolated from the elapsed wall-clock × playback rate.
    pub fn current_time(&self) -> f64 {
        if self.paused {
            self.video_time
        } else {
            self.video_time + self.captured_at.elapsed().as_secs_f64() * self.playback_rate
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn paused_sync(t: f64) -> TimelineSync {
        TimelineSync {
            video_time: t,
            captured_at: Instant::now(),
            paused: true,
            playback_rate: 1.0,
        }
    }

    #[test]
    fn current_time_fixed_when_paused() {
        let sync = paused_sync(42.5);
        assert_eq!(sync.current_time(), 42.5);
    }

    #[test]
    fn current_time_advances_when_playing() {
        let sync = TimelineSync {
            video_time: 10.0,
            captured_at: Instant::now() - Duration::from_secs(2),
            paused: false,
            playback_rate: 1.0,
        };
        let t = sync.current_time();
        assert!((12.0..12.1).contains(&t), "expected ~12.0, got {t}");
    }

    #[test]
    fn current_time_respects_playback_rate() {
        let sync = TimelineSync {
            video_time: 0.0,
            captured_at: Instant::now() - Duration::from_secs(2),
            paused: false,
            playback_rate: 2.0,
        };
        let t = sync.current_time();
        assert!((4.0..4.1).contains(&t), "2× speed: expected ~4.0, got {t}");
    }
}
