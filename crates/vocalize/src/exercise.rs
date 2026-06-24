//! Pure exercise logic: scales, item generation, note labels, and the pitch
//! matcher. No audio, no UI — fully unit-tested.

/// A musical scale kind. Indices are stable (persisted in config).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleKind {
    Major,
    NaturalMinor,
    Chromatic,
}

impl ScaleKind {
    pub fn index(self) -> usize {
        match self {
            ScaleKind::Major => 0,
            ScaleKind::NaturalMinor => 1,
            ScaleKind::Chromatic => 2,
        }
    }
    pub fn from_idx(i: usize) -> Self {
        match i {
            1 => ScaleKind::NaturalMinor,
            2 => ScaleKind::Chromatic,
            _ => ScaleKind::Major,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ScaleKind::Major => "Maior",
            ScaleKind::NaturalMinor => "Menor",
            ScaleKind::Chromatic => "Cromática",
        }
    }
    /// Semitone offsets of the scale degrees within one octave.
    pub fn degrees(self) -> &'static [i64] {
        match self {
            ScaleKind::Major => &[0, 2, 4, 5, 7, 9, 11],
            ScaleKind::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            ScaleKind::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }
    pub const ALL: [ScaleKind; 3] = [
        ScaleKind::Major,
        ScaleKind::NaturalMinor,
        ScaleKind::Chromatic,
    ];
}

/// Exercise mode. Indices stable (persisted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Notes,
    Chords,
}
impl Mode {
    pub fn index(self) -> usize {
        match self {
            Mode::Notes => 0,
            Mode::Chords => 1,
        }
    }
    pub fn from_idx(i: usize) -> Self {
        match i {
            1 => Mode::Chords,
            _ => Mode::Notes,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Mode::Notes => "Notas",
            Mode::Chords => "Acordes",
        }
    }
    pub const ALL: [Mode; 2] = [Mode::Notes, Mode::Chords];
}

/// A scale: a root pitch class (0–11) and a kind.
#[derive(Debug, Clone, Copy)]
pub struct Scale {
    pub root: i64,
    pub kind: ScaleKind,
}

impl Scale {
    /// MIDI number of the scale root in the playback octave (octave 4: C4 = 60).
    pub fn octave_root_midi(&self) -> i64 {
        60 + self.root.rem_euclid(12)
    }
    /// Number of degrees in this scale.
    pub fn degree_count(&self) -> usize {
        self.kind.degrees().len()
    }
}

/// Build the exercise item for a chosen scale-degree index.
///
/// `Mode::Notes` returns one MIDI note; `Mode::Chords` returns a three-note triad
/// stacked from that degree (scale degrees `d`, `d+2`, `d+4`, wrapping upward by an
/// octave so the notes ascend). All MIDI numbers are relative to the scale root in
/// the playback octave ([`Scale::octave_root_midi`]).
pub fn item_at(scale: &Scale, mode: Mode, degree: usize) -> Vec<i64> {
    let degs = scale.kind.degrees();
    let n = degs.len();
    let base = scale.octave_root_midi();
    let at = |i: usize| base + degs[i % n] + 12 * (i / n) as i64;
    let d = degree % n;
    match mode {
        Mode::Notes => vec![at(d)],
        Mode::Chords => vec![at(d), at(d + 2), at(d + 4)],
    }
}

const SOLFEGE: [&str; 12] = [
    "Dó", "Dó#", "Ré", "Ré#", "Mi", "Fá", "Fá#", "Sol", "Sol#", "Lá", "Lá#", "Si",
];

/// Display label for a MIDI note: solfège + letter, e.g. `note_label(60)` → "Dó (C)".
/// Octave is intentionally omitted (matching is octave-folded).
pub fn note_label(midi: i64) -> String {
    let (letter, _octave) = pitch::midi_name(midi);
    format!("{} ({})", SOLFEGE[midi.rem_euclid(12) as usize], letter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_kind_idx_roundtrips() {
        for k in ScaleKind::ALL {
            assert_eq!(ScaleKind::from_idx(k.index()), k);
        }
        assert_eq!(ScaleKind::from_idx(99), ScaleKind::Major);
    }

    #[test]
    fn mode_idx_roundtrips() {
        for m in Mode::ALL {
            assert_eq!(Mode::from_idx(m.index()), m);
        }
    }

    #[test]
    fn degree_counts() {
        assert_eq!(ScaleKind::Major.degrees().len(), 7);
        assert_eq!(ScaleKind::NaturalMinor.degrees().len(), 7);
        assert_eq!(ScaleKind::Chromatic.degrees().len(), 12);
    }

    #[test]
    fn octave_root_midi_maps_pitch_class() {
        assert_eq!(
            Scale {
                root: 0,
                kind: ScaleKind::Major
            }
            .octave_root_midi(),
            60
        );
        assert_eq!(
            Scale {
                root: 9,
                kind: ScaleKind::NaturalMinor
            }
            .octave_root_midi(),
            69
        );
    }

    #[test]
    fn note_item_picks_degree() {
        let s = Scale {
            root: 0,
            kind: ScaleKind::Major,
        };
        assert_eq!(item_at(&s, Mode::Notes, 0), vec![60]); // C4
        assert_eq!(item_at(&s, Mode::Notes, 4), vec![67]); // G4
    }

    #[test]
    fn chord_item_stacks_triad() {
        let s = Scale {
            root: 0,
            kind: ScaleKind::Major,
        };
        assert_eq!(item_at(&s, Mode::Chords, 0), vec![60, 64, 67]); // C E G
        assert_eq!(item_at(&s, Mode::Chords, 1), vec![62, 65, 69]); // D F A
    }

    #[test]
    fn chord_item_in_a_minor() {
        let s = Scale {
            root: 9,
            kind: ScaleKind::NaturalMinor,
        };
        assert_eq!(item_at(&s, Mode::Chords, 0), vec![69, 72, 76]); // A C E
    }

    #[test]
    fn note_label_is_solfege_and_letter() {
        assert_eq!(note_label(60), "Dó (C)");
        assert_eq!(note_label(69), "Lá (A)");
        assert_eq!(note_label(61), "Dó# (C#)");
    }
}
