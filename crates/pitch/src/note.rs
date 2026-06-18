//! Pure pitch → note math (no audio, no UI). Fully unit-tested.

/// Standard concert-pitch reference for A4, in Hz.
pub const A4: f64 = 440.0;

/// A detected note: name, octave, MIDI number, and cents deviation from perfect pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub name: &'static str,
    pub octave: i32,
    /// Nearest MIDI note number (69 = A4).
    pub midi: f64,
    pub cents: f64,
}

const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

impl Note {
    /// Build a note pinned to a fixed MIDI number with a known cents deviation.
    /// Used for instrument tuning, where the readout snaps to a target string
    /// rather than the nearest chromatic note.
    pub fn at_midi(midi: i64, cents: f64) -> Note {
        let (name, octave) = midi_name(midi);
        Note {
            name,
            octave,
            midi: midi as f64,
            cents,
        }
    }
}

/// The target MIDI note nearest to `freq` (Hz) under reference `a4`, with the
/// signed cents deviation from it. `targets` is an instrument's open-string MIDI
/// set; returns `None` if it is empty or `freq` is not positive.
pub fn nearest_target(freq: f64, a4: f64, targets: &[i64]) -> Option<(i64, f64)> {
    if targets.is_empty() || freq <= 0.0 {
        return None;
    }
    let frac = 69.0 + 12.0 * (freq / a4).log2();
    let best = targets
        .iter()
        .copied()
        .min_by(|&a, &b| (frac - a as f64).abs().total_cmp(&(frac - b as f64).abs()))?;
    Some((best, (frac - best as f64) * 100.0))
}

/// Nearest note to `freq` (Hz), with reference `a4` (usually [`A4`]).
pub fn frequency_to_note(freq: f64, a4: f64) -> Note {
    let midi = 69.0 + 12.0 * (freq / a4).log2();
    let rounded = midi.round();
    let cents = (midi - rounded) * 100.0;
    let semitone = rounded as i64;
    let name = NAMES[semitone.rem_euclid(12) as usize];
    let octave = (semitone.div_euclid(12) - 1) as i32; // MIDI: note 0 = C-1
    Note {
        name,
        octave,
        midi: rounded,
        cents,
    }
}

/// Frequency (Hz) of a MIDI note number, with reference `a4` (usually [`A4`]).
/// Inverse of the `midi` computed in [`frequency_to_note`].
pub fn note_to_frequency(midi: f64, a4: f64) -> f64 {
    a4 * 2f64.powf((midi - 69.0) / 12.0)
}

/// Name + octave of a MIDI note number (60 = C4), e.g. `67 -> ("G", 4)`.
pub fn midi_name(midi: i64) -> (&'static str, i32) {
    (
        NAMES[midi.rem_euclid(12) as usize],
        (midi.div_euclid(12) - 1) as i32,
    )
}

/// Whether the deviation is small enough to call "in tune".
pub fn is_in_tune(cents: f64) -> bool {
    cents.abs() < 5.0
}

#[cfg(test)]
mod tests {
    use super::*;
    fn approx(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn a4_is_440() {
        let n = frequency_to_note(440.0, 440.0);
        assert_eq!(n.name, "A");
        assert_eq!(n.octave, 4);
        assert_eq!(n.midi, 69.0);
        assert!(approx(n.cents, 0.0, 0.01), "cents {}", n.cents);
    }
    #[test]
    fn middle_c_is_c4() {
        let n = frequency_to_note(261.626, 440.0);
        assert_eq!(n.name, "C");
        assert_eq!(n.octave, 4);
        assert_eq!(n.midi, 60.0);
        assert!(approx(n.cents, 0.0, 1.0), "cents {}", n.cents);
    }
    #[test]
    fn a_sharp_4() {
        let n = frequency_to_note(466.164, 440.0);
        assert_eq!(n.name, "A#");
        assert_eq!(n.octave, 4);
    }
    #[test]
    fn slightly_sharp_is_positive_cents() {
        let n = frequency_to_note(445.0, 440.0);
        assert_eq!(n.name, "A");
        assert!(n.cents > 0.0 && n.cents < 50.0, "cents {}", n.cents);
    }
    #[test]
    fn in_tune_threshold() {
        assert!(is_in_tune(3.0));
        assert!(is_in_tune(-4.9));
        assert!(!is_in_tune(10.0));
    }
    #[test]
    fn midi_name_maps_numbers() {
        assert_eq!(midi_name(60), ("C", 4));
        assert_eq!(midi_name(69), ("A", 4));
        assert_eq!(midi_name(67), ("G", 4));
        assert_eq!(midi_name(57), ("A", 3));
    }
    #[test]
    fn note_to_frequency_inverts() {
        // A4 ⇒ 440, middle C ⇒ ~261.63.
        assert!(approx(note_to_frequency(69.0, 440.0), 440.0, 1e-9));
        assert!(approx(note_to_frequency(60.0, 440.0), 261.626, 0.01));
    }

    // Standard guitar open strings: E2 A2 D3 G3 B3 E4.
    const GUITAR: [i64; 6] = [40, 45, 50, 55, 59, 64];

    #[test]
    fn nearest_target_picks_closest_string() {
        // Low E string ≈ 82.41 Hz at A440 → E2 (midi 40), ~0¢.
        let (midi, cents) = nearest_target(82.41, 440.0, &GUITAR).unwrap();
        assert_eq!(midi, 40);
        assert!(approx(cents, 0.0, 1.0), "cents {cents}");
        assert_eq!(midi_name(midi), ("E", 2));
    }

    #[test]
    fn nearest_target_signs_cents() {
        // A bit sharp of A2 (110 Hz) → A2 (midi 45) with positive cents.
        let (midi, cents) = nearest_target(112.0, 440.0, &GUITAR).unwrap();
        assert_eq!(midi, 45);
        assert!(cents > 0.0 && cents < 50.0, "cents {cents}");
    }

    #[test]
    fn nearest_target_reference_shifts_cents() {
        // The same input Hz reads differently against a 432 Hz reference.
        let (m440, c440) = nearest_target(110.0, 440.0, &GUITAR).unwrap();
        let (m432, c432) = nearest_target(110.0, 432.0, &GUITAR).unwrap();
        assert_eq!(m440, 45);
        assert_eq!(m432, 45);
        assert!((c440 - c432).abs() > 10.0, "{c440} vs {c432}");
    }

    #[test]
    fn nearest_target_empty_or_invalid_is_none() {
        assert!(nearest_target(110.0, 440.0, &[]).is_none());
        assert!(nearest_target(0.0, 440.0, &GUITAR).is_none());
        assert!(nearest_target(-5.0, 440.0, &GUITAR).is_none());
    }

    #[test]
    fn at_midi_derives_name_and_octave() {
        let n = Note::at_midi(40, 12.0);
        assert_eq!((n.name, n.octave), ("E", 2));
        assert_eq!(n.midi, 40.0);
        assert!(approx(n.cents, 12.0, 1e-9));
    }
}
