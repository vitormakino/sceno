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

/// Exercise mode: how many/which notes make up each item. Indices stable (persisted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Notes,
    PowerChord,
    Triad,
    Tetrad,
}
impl Mode {
    pub fn index(self) -> usize {
        match self {
            Mode::Notes => 0,
            Mode::PowerChord => 1,
            Mode::Triad => 2,
            Mode::Tetrad => 3,
        }
    }
    pub fn from_idx(i: usize) -> Self {
        match i {
            1 => Mode::PowerChord,
            2 => Mode::Triad,
            3 => Mode::Tetrad,
            _ => Mode::Notes,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Mode::Notes => "Notas",
            Mode::PowerChord => "Power chord",
            Mode::Triad => "Tríade",
            Mode::Tetrad => "Tétrade",
        }
    }
    pub const ALL: [Mode; 4] = [Mode::Notes, Mode::PowerChord, Mode::Triad, Mode::Tetrad];
}

/// How a chord's reference tone is played: all notes at once, or one after another.
/// Indices stable (persisted). Single-note items ignore this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayStyle {
    Together,
    Arpeggio,
}
impl PlayStyle {
    pub fn index(self) -> usize {
        match self {
            PlayStyle::Together => 0,
            PlayStyle::Arpeggio => 1,
        }
    }
    pub fn from_idx(i: usize) -> Self {
        match i {
            1 => PlayStyle::Arpeggio,
            _ => PlayStyle::Together,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            PlayStyle::Together => "Junto",
            PlayStyle::Arpeggio => "Arpejo",
        }
    }
    pub const ALL: [PlayStyle; 2] = [PlayStyle::Together, PlayStyle::Arpeggio];
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

/// Build the exercise item (the MIDI notes to sing) for a chosen scale-degree index.
///
/// All notes are relative to the scale root in the playback octave
/// ([`Scale::octave_root_midi`]). Chord shapes other than the power chord are built from
/// scale degrees `d`, `d+2`, … (every other step), wrapping upward by an octave so the
/// notes ascend:
/// - [`Mode::Notes`] — one note, `at(d)`.
/// - [`Mode::PowerChord`] — root + a *perfect* fifth, `[at(d), at(d) + 7]`. The fifth is an
///   absolute 7-semitone interval (not a scale degree), so it stays perfect on every degree
///   and scale — the defining trait of a power chord.
/// - [`Mode::Triad`] — `[at(d), at(d+2), at(d+4)]` (diatonic triad).
/// - [`Mode::Tetrad`] — `[at(d), at(d+2), at(d+4), at(d+6)]` (diatonic seventh chord).
pub fn item_at(scale: &Scale, mode: Mode, degree: usize) -> Vec<i64> {
    let degs = scale.kind.degrees();
    let n = degs.len();
    let base = scale.octave_root_midi();
    let at = |i: usize| base + degs[i % n] + 12 * (i / n) as i64;
    let d = degree % n;
    match mode {
        Mode::Notes => vec![at(d)],
        Mode::PowerChord => vec![at(d), at(d) + 7],
        Mode::Triad => vec![at(d), at(d + 2), at(d + 4)],
        Mode::Tetrad => vec![at(d), at(d + 2), at(d + 4), at(d + 6)],
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

/// Tracks how long each still-uncollected target has been sung in-tune and reports
/// targets as "collected" once held for the sustain time. Matching is octave-folded:
/// only the pitch class matters.
pub struct Matcher {
    classes: Vec<i64>,
    held_ms: Vec<f64>,
    collected: Vec<bool>,
    cents_window: f64,
    sustain_ms: f64,
}

impl Matcher {
    pub fn new(item: &[i64], cents_window: f64, sustain_ms: f64) -> Self {
        let classes: Vec<i64> = item.iter().map(|m| m.rem_euclid(12)).collect();
        let len = classes.len();
        Matcher {
            classes,
            held_ms: vec![0.0; len],
            collected: vec![false; len],
            cents_window,
            sustain_ms,
        }
    }

    /// Feed one analysis frame. `sung` is a continuous MIDI value (note + cents/100)
    /// or `None` on silence; `dt_ms` is the time since the previous frame. Returns
    /// the indices that became collected on this frame.
    pub fn update(&mut self, sung: Option<f64>, dt_ms: f64) -> Vec<usize> {
        let mut newly = Vec::new();
        for i in 0..self.classes.len() {
            if self.collected[i] {
                continue;
            }
            let in_window = sung
                .map(|p| cents_from_class(p, self.classes[i]).abs() <= self.cents_window)
                .unwrap_or(false);
            if in_window {
                self.held_ms[i] += dt_ms;
                if self.held_ms[i] >= self.sustain_ms {
                    self.collected[i] = true;
                    newly.push(i);
                }
            } else {
                self.held_ms[i] = 0.0;
            }
        }
        newly
    }

    pub fn collected(&self) -> &[bool] {
        &self.collected
    }
    pub fn all_collected(&self) -> bool {
        self.collected.iter().all(|&c| c)
    }
}

/// Signed cents from a continuous MIDI pitch to the nearest octave of `class` (0–11).
fn cents_from_class(sung_midi: f64, class: i64) -> f64 {
    let mut diff = (sung_midi - class as f64).rem_euclid(12.0); // 0..12
    if diff > 6.0 {
        diff -= 12.0;
    }
    diff * 100.0
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
        assert_eq!(item_at(&s, Mode::Triad, 0), vec![60, 64, 67]); // C E G
        assert_eq!(item_at(&s, Mode::Triad, 1), vec![62, 65, 69]); // D F A
    }

    #[test]
    fn chord_item_in_a_minor() {
        let s = Scale {
            root: 9,
            kind: ScaleKind::NaturalMinor,
        };
        assert_eq!(item_at(&s, Mode::Triad, 0), vec![69, 72, 76]); // A C E
    }

    #[test]
    fn power_chord_is_perfect_fifth() {
        let s = Scale {
            root: 0,
            kind: ScaleKind::Major,
        };
        // C major, root: C + perfect fifth G.
        assert_eq!(item_at(&s, Mode::PowerChord, 0), vec![60, 67]);
        // Leading-tone degree (B): an absolute perfect fifth (B + F#), NOT the diatonic
        // diminished fifth (B + F) — this is what makes it a power chord.
        assert_eq!(item_at(&s, Mode::PowerChord, 6), vec![71, 78]);
    }

    #[test]
    fn tetrad_stacks_seventh() {
        let s = Scale {
            root: 0,
            kind: ScaleKind::Major,
        };
        // C major degree 0 → Cmaj7 (C E G B).
        assert_eq!(item_at(&s, Mode::Tetrad, 0), vec![60, 64, 67, 71]);
    }

    #[test]
    fn note_label_is_solfege_and_letter() {
        assert_eq!(note_label(60), "Dó (C)");
        assert_eq!(note_label(69), "Lá (A)");
        assert_eq!(note_label(61), "Dó# (C#)");
    }

    #[test]
    fn cents_from_class_signs() {
        assert!((cents_from_class(60.2, 0) - 20.0).abs() < 1e-6);
        assert!((cents_from_class(59.8, 0) + 20.0).abs() < 1e-6);
        // A tritone away is the max distance (±600¢).
        assert!((cents_from_class(66.0, 0).abs() - 600.0).abs() < 1e-6);
    }

    #[test]
    fn collects_after_sustain() {
        let mut m = Matcher::new(&[60], 50.0, 500.0);
        for _ in 0..4 {
            assert!(m.update(Some(60.0), 100.0).is_empty());
        }
        assert_eq!(m.update(Some(60.0), 100.0), vec![0]); // 500ms reached
        assert!(m.all_collected());
        // Already collected → never returned again.
        assert!(m.update(Some(60.0), 100.0).is_empty());
    }

    #[test]
    fn out_of_window_resets_hold() {
        let mut m = Matcher::new(&[60], 50.0, 500.0);
        m.update(Some(60.0), 300.0);
        m.update(Some(63.0), 100.0); // 3 semitones off → resets
        // Now needs a full 500ms again.
        for _ in 0..4 {
            assert!(m.update(Some(60.0), 100.0).is_empty());
        }
        assert_eq!(m.update(Some(60.0), 100.0), vec![0]);
    }

    #[test]
    fn silence_resets_hold() {
        let mut m = Matcher::new(&[60], 50.0, 500.0);
        m.update(Some(60.0), 300.0);
        m.update(None, 100.0);
        assert!(!m.all_collected());
    }

    #[test]
    fn matching_is_octave_folded() {
        let mut m = Matcher::new(&[60], 50.0, 500.0); // target C
        // Singing C an octave up (72) still matches the pitch class.
        assert_eq!(m.update(Some(72.0), 500.0), vec![0]);
    }

    #[test]
    fn chord_collects_each_target_independently() {
        let mut m = Matcher::new(&[60, 64, 67], 50.0, 100.0); // C E G
        assert_eq!(m.update(Some(64.0), 100.0), vec![1]); // sing E
        assert_eq!(m.update(Some(60.0), 100.0), vec![0]); // sing C
        assert!(!m.all_collected());
        assert_eq!(m.update(Some(67.0), 100.0), vec![2]); // sing G
        assert!(m.all_collected());
    }
}
