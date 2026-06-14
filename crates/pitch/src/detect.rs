//! Fundamental-frequency estimation (McLeod / MPM), pure over a sample buffer.

use pitch_detection::detector::PitchDetector;
use pitch_detection::detector::mcleod::McLeodDetector;

/// Estimate the fundamental frequency of a mono f32 buffer, or `None` if no
/// clear pitch. `sample_rate` in Hz; `min_clarity` in 0..1.
pub fn detect_frequency(samples: &[f32], sample_rate: u32, min_clarity: f64) -> Option<f64> {
    let size = samples.len();
    if size < 256 {
        return None;
    }
    let padding = size / 2;
    let signal: Vec<f64> = samples.iter().map(|&s| s as f64).collect();
    let mut detector = McLeodDetector::new(size, padding);
    detector
        .get_pitch(&signal, sample_rate as usize, 0.15, min_clarity)
        .map(|p| p.frequency)
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
}
