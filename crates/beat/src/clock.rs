//! The shared beat clock: a constant tempo with a downbeat phase anchor.
//!
//! [`SharedClock`] is cloned across the UI thread, the audio click thread, and
//! the detection thread; all reads/writes go through atomics so it is the single
//! source of truth that keeps the audible click and the visual flash in lockstep.
//! Phase is anchored to an [`Instant`]: beat index 0 (a downbeat) occurs at the
//! anchor, and `beat_position_at(t)` returns the fractional beats elapsed since.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

/// Lowest tempo the clock will hold (also the tray's clamp floor).
pub const MIN_BPM: f64 = 30.0;
/// Highest tempo the clock will hold.
pub const MAX_BPM: f64 = 300.0;

/// Fractional beats between an anchor downbeat and a point `elapsed_secs` later.
pub fn beat_position(elapsed_secs: f64, bpm: f64) -> f64 {
    elapsed_secs * bpm / 60.0
}

/// Estimate BPM from successive tap intervals (seconds), discarding implausible
/// gaps (<50 ms or >3 s) so a stray double-tap or a long pause doesn't poison the
/// average. Returns `None` when no interval survives.
pub fn tap_bpm(intervals: &[f64]) -> Option<f64> {
    let valid: Vec<f64> = intervals
        .iter()
        .copied()
        .filter(|&d| (0.05..=3.0).contains(&d))
        .collect();
    if valid.is_empty() {
        return None;
    }
    let mean = valid.iter().sum::<f64>() / valid.len() as f64;
    (mean > 0.0).then(|| (60.0 / mean).clamp(MIN_BPM, MAX_BPM))
}

struct Inner {
    /// Process-start reference for the nanosecond anchor below.
    base: Instant,
    /// BPM × 1000, so tempo survives as an integer atomic.
    bpm_milli: AtomicU64,
    beats_per_bar: AtomicU32,
    /// Nanoseconds from `base` to the beat-0 downbeat (may be negative).
    anchor_nanos: AtomicI64,
    running: AtomicBool,
    audible: AtomicBool,
}

/// A cheap-to-clone handle to the shared beat clock.
#[derive(Clone)]
pub struct SharedClock(Arc<Inner>);

impl SharedClock {
    /// Create a clock at `bpm`/`beats_per_bar`, stopped, with audio enabled and
    /// the downbeat anchored at creation time.
    pub fn new(bpm: f64, beats_per_bar: u32) -> Self {
        SharedClock(Arc::new(Inner {
            base: Instant::now(),
            bpm_milli: AtomicU64::new(milli(bpm)),
            beats_per_bar: AtomicU32::new(beats_per_bar.max(1)),
            anchor_nanos: AtomicI64::new(0),
            running: AtomicBool::new(false),
            audible: AtomicBool::new(true),
        }))
    }

    pub fn bpm(&self) -> f64 {
        self.0.bpm_milli.load(Ordering::Relaxed) as f64 / 1000.0
    }
    pub fn set_bpm(&self, bpm: f64) {
        self.0.bpm_milli.store(milli(bpm), Ordering::Relaxed);
    }
    pub fn beats_per_bar(&self) -> u32 {
        self.0.beats_per_bar.load(Ordering::Relaxed).max(1)
    }
    pub fn set_beats_per_bar(&self, n: u32) {
        self.0.beats_per_bar.store(n.max(1), Ordering::Relaxed);
    }
    pub fn running(&self) -> bool {
        self.0.running.load(Ordering::Relaxed)
    }
    pub fn set_running(&self, on: bool) {
        self.0.running.store(on, Ordering::Relaxed);
    }
    pub fn audible(&self) -> bool {
        self.0.audible.load(Ordering::Relaxed)
    }
    pub fn set_audible(&self, on: bool) {
        self.0.audible.store(on, Ordering::Relaxed);
    }

    /// Nanoseconds from `base` to `t` (clamped at 0; the clock is created first).
    fn rel_nanos(&self, t: Instant) -> i64 {
        t.saturating_duration_since(self.0.base).as_nanos() as i64
    }

    /// Make `t` an exact downbeat (beat index 0) while keeping the tempo.
    pub fn anchor_to(&self, t: Instant) {
        self.0
            .anchor_nanos
            .store(self.rel_nanos(t), Ordering::Relaxed);
    }

    /// Align the phase so the beat position at `t` equals `beat` fractional beats
    /// from a downbeat — used to lock onto a song's or a detected grid.
    pub fn rephase(&self, t: Instant, beat: f64) {
        let back_secs = beat * 60.0 / self.bpm();
        let anchor = self.rel_nanos(t) - (back_secs * 1e9) as i64;
        self.0.anchor_nanos.store(anchor, Ordering::Relaxed);
    }

    /// Fractional beats since the anchor downbeat at instant `t`.
    pub fn beat_position_at(&self, t: Instant) -> f64 {
        let elapsed =
            (self.rel_nanos(t) - self.0.anchor_nanos.load(Ordering::Relaxed)) as f64 / 1e9;
        beat_position(elapsed, self.bpm())
    }
}

fn milli(bpm: f64) -> u64 {
    (bpm.clamp(MIN_BPM, MAX_BPM) * 1000.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn beat_position_scales_with_tempo() {
        // 120 BPM → 2 beats/sec.
        assert!((beat_position(1.0, 120.0) - 2.0).abs() < 1e-9);
        assert!((beat_position(0.5, 120.0) - 1.0).abs() < 1e-9);
        // 60 BPM → 1 beat/sec.
        assert!((beat_position(3.0, 60.0) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn tap_bpm_averages_intervals() {
        // 0.5 s gaps → 120 BPM.
        assert!((tap_bpm(&[0.5, 0.5, 0.5]).unwrap() - 120.0).abs() < 1e-6);
        // A stray tiny/huge interval is discarded, not averaged in.
        assert!((tap_bpm(&[0.5, 0.001, 10.0, 0.5]).unwrap() - 120.0).abs() < 1e-6);
        assert!(tap_bpm(&[]).is_none());
    }

    #[test]
    fn tap_bpm_clamps_to_range() {
        // 10 ms gaps would be 6000 BPM but are filtered out; a 0.1 s gap → 600 → clamped.
        assert_eq!(tap_bpm(&[0.1]).unwrap(), MAX_BPM);
    }

    #[test]
    fn rephase_then_position_is_consistent() {
        let clock = SharedClock::new(120.0, 4);
        let now = Instant::now();
        // Declare "now" to be 1.5 beats past a downbeat.
        clock.rephase(now, 1.5);
        let pos = clock.beat_position_at(now);
        assert!((pos - 1.5).abs() < 1e-3, "got {pos}");
        // Half a beat later (0.25 s at 120 BPM) we should be at ~2.0 beats.
        let later = now + Duration::from_millis(250);
        let pos2 = clock.beat_position_at(later);
        assert!((pos2 - 2.0).abs() < 5e-3, "got {pos2}");
    }

    #[test]
    fn anchor_to_makes_a_downbeat() {
        let clock = SharedClock::new(100.0, 4);
        let now = Instant::now();
        clock.anchor_to(now);
        assert!(clock.beat_position_at(now).abs() < 1e-3);
    }
}
