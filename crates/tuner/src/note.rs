//! Pure pitch → note math (no audio, no UI). Fully unit-tested.

/// A detected note: name, octave, and cents deviation from perfect pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub name: &'static str,
    pub octave: i32,
    pub cents: f64,
}

const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Nearest note to `freq` (Hz), with reference `a4` (usually 440.0).
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
        cents,
    }
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
        assert!(approx(n.cents, 0.0, 0.01), "cents {}", n.cents);
    }
    #[test]
    fn middle_c_is_c4() {
        let n = frequency_to_note(261.626, 440.0);
        assert_eq!(n.name, "C");
        assert_eq!(n.octave, 4);
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
}
