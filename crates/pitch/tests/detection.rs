//! Integration test for the real detection algorithm against *voice-like* signals
//! (a fundamental plus decaying harmonics, optionally with vibrato and noise),
//! swept across the singing range at both common sample rates, using the
//! production `MIN_CLARITY` / `WINDOW`. The mic can't run in CI, but this drives
//! the exact `detect_frequency` path the app uses, so it objectively measures
//! detection accuracy (octave-folded, like the vocalize matcher).

use pitch::{A4, MIN_CLARITY, WINDOW, detect_frequency, frequency_to_note, note_to_frequency};
use std::f32::consts::PI;

/// Synthesize a sustained, voice-like tone at `midi`, phase-accurate so vibrato
/// is correct. Harmonic amplitudes roughly mimic a sung vowel.
fn voice(midi: f64, sr: u32, n: usize, vibrato_cents: f32, noise_amp: f32) -> Vec<f32> {
    let f0 = note_to_frequency(midi, A4) as f32;
    let harmonics = [1.0f32, 0.6, 0.4, 0.28, 0.2, 0.12];
    let mut phase = 0.0f32;
    let mut rng = 0x9E37_79B9u32;
    (0..n)
        .map(|i| {
            let t = i as f32 / sr as f32;
            // 5.5 Hz vibrato, ±vibrato_cents around f0.
            let vib = if vibrato_cents > 0.0 {
                2f32.powf((vibrato_cents / 1200.0) * (2.0 * PI * 5.5 * t).sin())
            } else {
                1.0
            };
            phase += 2.0 * PI * f0 * vib / sr as f32;
            let tone: f32 = harmonics
                .iter()
                .enumerate()
                .map(|(h, &a)| a * (phase * (h as f32 + 1.0)).sin())
                .sum();
            // Cheap LCG white noise.
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            let nz = ((rng >> 8) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0;
            tone * 0.2 + nz * noise_amp
        })
        .collect()
}

/// Sweep the given scenario across A2..C5 at 44.1 and 48 kHz; return human-readable
/// failures (no detection, or wrong octave-folded pitch class beyond tolerance).
fn sweep(label: &str, vibrato_cents: f32, noise_amp: f32, cents_tol: f64) -> Vec<String> {
    let mut fails = Vec::new();
    for &sr in &[44_100u32, 48_000] {
        for midi in 45..=72 {
            // A2 (45) .. C5 (72)
            let buf = voice(midi as f64, sr, WINDOW, vibrato_cents, noise_amp);
            match detect_frequency(&buf, sr, MIN_CLARITY) {
                None => fails.push(format!("[{label}] sr{sr} midi{midi}: NO DETECTION")),
                Some(f) => {
                    let det = frequency_to_note(f, A4);
                    let tgt_class = (midi as i64).rem_euclid(12);
                    let det_class = (det.midi as i64).rem_euclid(12);
                    // Octave-folded cents error to the target pitch class.
                    let target_hz = note_to_frequency(midi as f64, A4);
                    let mut semis = 12.0 * (f / target_hz).log2();
                    semis -= (semis / 12.0).round() * 12.0; // fold octaves
                    let cents = semis.abs() * 100.0;
                    if det_class != tgt_class || cents > cents_tol {
                        fails.push(format!(
                            "[{label}] sr{sr} midi{midi}: got {f:.1}Hz ({}{}, {cents:.0}¢ off-class)",
                            det.name, det.octave
                        ));
                    }
                }
            }
        }
    }
    fails
}

/// Detection rate (% of notes in A2..C5, both SRs) for a given noise level and
/// clarity threshold — data for choosing `MIN_CLARITY`.
fn detection_rate(noise_amp: f32, clarity: f64) -> f64 {
    let mut total = 0;
    let mut ok = 0;
    for &sr in &[44_100u32, 48_000] {
        for midi in 45..=72 {
            total += 1;
            let buf = voice(midi as f64, sr, WINDOW, 0.0, noise_amp);
            if let Some(f) = detect_frequency(&buf, sr, clarity) {
                let target_hz = note_to_frequency(midi as f64, A4);
                let mut semis = 12.0 * (f / target_hz).log2();
                semis -= (semis / 12.0).round() * 12.0;
                if semis.abs() * 100.0 <= 50.0 {
                    ok += 1;
                }
            }
        }
    }
    100.0 * ok as f64 / total as f64
}

/// Pure noise (no tone), used to confirm the clarity gate doesn't invent a pitch.
fn noise_only(n: usize, amp: f32, seed: u32) -> Vec<f32> {
    let mut rng = seed | 1;
    (0..n)
        .map(|_| {
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            (((rng >> 8) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0) * amp
        })
        .collect()
}

#[test]
fn noisy_voice_is_detected_at_production_clarity() {
    // The whole point of MIN_CLARITY = 0.4: a voiced tone buried in noise
    // (amp 0.3, comparable to the signal) must still be detected. At the old 0.6
    // this was 0%. Guards against a regression that re-breaks real mics.
    let rate = detection_rate(0.3, MIN_CLARITY);
    assert!(
        rate >= 90.0,
        "noisy-voice detection rate {rate:.0}% (want ≥90)"
    );
}

#[test]
fn pure_noise_is_mostly_rejected_at_production_clarity() {
    // Lowering the clarity gate must not make silence/noise read as a pitch.
    let mut detections = 0;
    let total = 16;
    for seed in 0..total as u32 {
        let buf = noise_only(WINDOW, 0.3, seed.wrapping_mul(2_654_435_761));
        if detect_frequency(&buf, 48_000, MIN_CLARITY).is_some() {
            detections += 1;
        }
    }
    assert!(
        detections * 5 <= total, // ≤20% false-positive on pure noise
        "pure noise produced {detections}/{total} false detections"
    );
}

#[test]
fn detects_clean_harmonic_voice_across_range() {
    let fails = sweep("clean", 0.0, 0.0, 35.0);
    assert!(
        fails.is_empty(),
        "{} failures:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

#[test]
fn detects_voice_with_vibrato() {
    // ±35¢ vibrato is typical; the detector should still land near the center.
    let fails = sweep("vibrato", 35.0, 0.0, 45.0);
    assert!(
        fails.is_empty(),
        "{} failures:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

#[test]
fn detects_voice_with_light_noise() {
    let fails = sweep("noise", 0.0, 0.05, 40.0);
    assert!(
        fails.is_empty(),
        "{} failures:\n{}",
        fails.len(),
        fails.join("\n")
    );
}
