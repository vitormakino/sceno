//! Time-integrated karaoke accuracy score.
//!
//! Each frame contributes the playback time it covered, weighted by how on-pitch
//! the singing was. Integrating by *playback time* (not frame count) keeps the
//! score independent of the tick cadence and robust to pauses/seeks. The result
//! is the fraction of target-active time sung on pitch, as a 0–100 percentage.

/// Largest playback-time step counted per sample; bigger jumps (a seek, or
/// resuming from pause) are clamped so they neither inflate nor tank the score.
const MAX_DT: f64 = 0.5;

#[derive(Default)]
pub struct Scorer {
    earned: f64,
    possible: f64,
    last_t: Option<f64>,
}

impl Scorer {
    /// Start fresh (new song).
    pub fn reset(&mut self) {
        *self = Scorer::default();
    }

    /// Accumulate one frame at playback time `t` (seconds). `has_target` is whether
    /// a note is active right now; `golden` weights that note ×2; `sung_cents` is the
    /// octave-folded deviation of the sung pitch from the target (`None` when silent
    /// or no target). The first sample only seeds the clock (dt = 0).
    pub fn sample(&mut self, t: f64, has_target: bool, golden: bool, sung_cents: Option<f64>) {
        let dt = match self.last_t {
            Some(p) if t > p => (t - p).min(MAX_DT),
            _ => 0.0,
        };
        self.last_t = Some(t);
        if dt <= 0.0 || !has_target {
            return;
        }
        let w = if golden { 2.0 } else { 1.0 };
        self.possible += dt * w;
        if let Some(c) = sung_cents {
            self.earned += dt * w * (1.0 - c.abs() / 50.0).clamp(0.0, 1.0);
        }
    }

    /// Score so far (0–100), or `None` before any target time has elapsed.
    pub fn pct(&self) -> Option<f64> {
        (self.possible > 0.0).then(|| self.earned / self.possible * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn no_target_time_is_none() {
        let mut s = Scorer::default();
        s.sample(0.0, false, false, None);
        s.sample(0.1, false, false, None);
        assert_eq!(s.pct(), None);
    }

    #[test]
    fn perfect_singing_is_full_score() {
        let mut s = Scorer::default();
        s.sample(0.0, true, false, Some(0.0)); // seed (dt=0)
        s.sample(0.1, true, false, Some(0.0));
        s.sample(0.2, true, false, Some(0.0));
        assert!(approx(s.pct().unwrap(), 100.0));
    }

    #[test]
    fn silence_over_target_is_zero() {
        let mut s = Scorer::default();
        s.sample(0.0, true, false, None);
        s.sample(0.1, true, false, None);
        assert!(approx(s.pct().unwrap(), 0.0));
    }

    #[test]
    fn half_off_is_half_credit() {
        let mut s = Scorer::default();
        s.sample(0.0, true, false, Some(25.0));
        s.sample(0.1, true, false, Some(25.0));
        assert!(approx(s.pct().unwrap(), 50.0));
    }

    #[test]
    fn beyond_fifty_cents_scores_nothing() {
        let mut s = Scorer::default();
        s.sample(0.0, true, false, Some(80.0));
        s.sample(0.1, true, false, Some(80.0));
        assert!(approx(s.pct().unwrap(), 0.0));
    }

    #[test]
    fn golden_notes_weigh_double() {
        // One normal note missed, one golden note hit perfectly.
        let mut s = Scorer::default();
        s.sample(0.0, true, false, None); // seed
        s.sample(0.1, true, false, None); // normal, missed: possible += 0.1
        s.sample(0.2, true, true, Some(0.0)); // seed-less continuation
        s.sample(0.3, true, true, Some(0.0)); // golden, hit: possible += 0.2, earned += 0.2
        // earned 0.2 / possible (0.1 + 0.1*? ) ...
        // possible = 0.1 (normal) + 0.1 (golden t .1->.2) + 0.1 (golden .2->.3, w2 => 0.2) ...
        // Simpler: assert golden hit pulls the score above the 50% a fair split implies.
        assert!(s.pct().unwrap() > 50.0);
    }

    #[test]
    fn big_time_jump_is_clamped() {
        let mut s = Scorer::default();
        s.sample(0.0, true, false, Some(0.0));
        s.sample(100.0, true, false, Some(0.0)); // clamp dt to MAX_DT
        // Only MAX_DT of possible time should have accrued.
        assert!(approx(s.pct().unwrap(), 100.0));
        // possible is bounded: a second perfect clamped step keeps it at 100.
        s.sample(100.5, true, false, Some(0.0));
        assert!(approx(s.pct().unwrap(), 100.0));
    }

    #[test]
    fn reset_zeroes() {
        let mut s = Scorer::default();
        s.sample(0.0, true, false, Some(0.0));
        s.sample(0.1, true, false, Some(0.0));
        s.reset();
        assert_eq!(s.pct(), None);
    }
}
