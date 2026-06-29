//! Fundamental-frequency estimation via **pYIN** (probabilistic YIN), pure Rust.
//!
//! Replaces the earlier McLeod/MPM detector, which — on a sensitive condenser mic
//! — locked onto sub-bass rumble (~30 Hz) or upper partials (~2 kHz) instead of
//! the sung note. pYIN (Mauch & Dixon 2014) searches only within `[MIN_HZ, MAX_HZ]`
//! and tracks the most probable pitch, so it's far more robust to exactly those
//! octave/harmonic errors and to breathy/noisy voice.

use pyin::{Framing, PYINExecutor, PadMode};
use std::cell::RefCell;

/// Plausible sung-pitch range (Hz), handed to pYIN as fmin/fmax (~C#2 to ~C6).
/// Searching only this band means sub-bass rumble and high harmonics can't be
/// mistaken for the fundamental.
pub const MIN_HZ: f64 = 70.0;
pub const MAX_HZ: f64 = 1000.0;

/// pYIN analysis frame (samples); the capture `WINDOW` holds several of these,
/// which pYIN tracks and we reduce to a single robust estimate.
const FRAME: usize = 2048;

thread_local! {
    /// Reused pYIN executor (keyed by sample rate). Building it allocates FFT
    /// plans + HMM matrices, so creating one per 50 ms frame is far too slow —
    /// the capture loop calls this on one thread, so a thread-local cache fits.
    static EXECUTOR: RefCell<Option<(u32, PYINExecutor<f32>)>> = const { RefCell::new(None) };
}

/// Estimate the fundamental frequency of a mono f32 buffer, or `None` if no
/// confident voiced pitch in range. `sample_rate` in Hz; `min_clarity` (0..1) is
/// a floor on pYIN's voicing probability, on top of its own voiced/unvoiced HMM.
pub fn detect_frequency(samples: &[f32], sample_rate: u32, min_clarity: f64) -> Option<f64> {
    if samples.len() < FRAME {
        return None;
    }
    EXECUTOR.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.as_ref().map(|(sr, _)| *sr) != Some(sample_rate) {
            *slot = Some((
                sample_rate,
                PYINExecutor::new(MIN_HZ, MAX_HZ, sample_rate, FRAME, None, None, None),
            ));
        }
        let exec = &mut slot.as_mut().unwrap().1;
        let (_timestamps, f0, voiced_flag, voiced_prob) = exec.pyin(
            samples,
            f32::NAN,
            Framing::Center(PadMode::Constant(0.0f32)),
        );

        // Median f0 over voiced frames (pYIN's own HMM decision, plus a probability
        // floor) — robust to the odd outlier frame; unvoiced frames are NaN.
        let mut hz: Vec<f64> = Vec::with_capacity(f0.len());
        for i in 0..f0.len() {
            if voiced_flag[i] && (voiced_prob[i] as f64) >= min_clarity && f0[i].is_finite() {
                hz.push(f0[i] as f64);
            }
        }
        if hz.is_empty() {
            return None;
        }
        hz.sort_by(f64::total_cmp);
        Some(hz[hz.len() / 2])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine(freq: f32, sr: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f32 / sr as f32).sin())
            .collect()
    }

    #[test]
    fn detects_a440_sine() {
        let sr = 44_100;
        let buf = sine(440.0, sr, 4096);
        let f = detect_frequency(&buf, sr, 0.5).expect("should detect a clear sine");
        assert!((f - 440.0).abs() < 5.0, "got {f}");
    }

    #[test]
    fn silence_has_no_pitch() {
        let buf = vec![0.0f32; 4096];
        assert!(detect_frequency(&buf, 44_100, 0.5).is_none());
    }

    #[test]
    fn rejects_out_of_range_subbass() {
        // A 32 Hz rumble (below MIN_HZ) must not be reported — the real-mic failure.
        let sr = 48_000;
        let buf = sine(32.0, sr, 4096);
        assert!(
            detect_frequency(&buf, sr, 0.4).is_none(),
            "subbass leaked through"
        );
    }
}
