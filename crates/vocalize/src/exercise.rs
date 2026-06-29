//! Pure exercise logic: scales, item generation, note labels, and the pitch
//! matcher. No audio, no UI — fully unit-tested.

/// A musical scale kind. Indices are stable and append-only (persisted in
/// config), so new kinds must be added at the end to keep existing configs valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleKind {
    Major,
    NaturalMinor,
    Chromatic,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
    HarmonicMinor,
    MelodicMinor,
}

impl ScaleKind {
    pub fn index(self) -> usize {
        match self {
            ScaleKind::Major => 0,
            ScaleKind::NaturalMinor => 1,
            ScaleKind::Chromatic => 2,
            ScaleKind::Dorian => 3,
            ScaleKind::Phrygian => 4,
            ScaleKind::Lydian => 5,
            ScaleKind::Mixolydian => 6,
            ScaleKind::Locrian => 7,
            ScaleKind::HarmonicMinor => 8,
            ScaleKind::MelodicMinor => 9,
        }
    }
    pub fn from_idx(i: usize) -> Self {
        match i {
            1 => ScaleKind::NaturalMinor,
            2 => ScaleKind::Chromatic,
            3 => ScaleKind::Dorian,
            4 => ScaleKind::Phrygian,
            5 => ScaleKind::Lydian,
            6 => ScaleKind::Mixolydian,
            7 => ScaleKind::Locrian,
            8 => ScaleKind::HarmonicMinor,
            9 => ScaleKind::MelodicMinor,
            _ => ScaleKind::Major,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ScaleKind::Major => "Maior",
            ScaleKind::NaturalMinor => "Menor",
            ScaleKind::Chromatic => "Cromática",
            ScaleKind::Dorian => "Dórico",
            ScaleKind::Phrygian => "Frígio",
            ScaleKind::Lydian => "Lídio",
            ScaleKind::Mixolydian => "Mixolídio",
            ScaleKind::Locrian => "Lócrio",
            ScaleKind::HarmonicMinor => "Menor harmônica",
            ScaleKind::MelodicMinor => "Menor melódica",
        }
    }
    /// Semitone offsets of the scale degrees within one octave. The modes and
    /// minor variants are 7-degree; the melodic minor is its ascending form.
    pub fn degrees(self) -> &'static [i64] {
        match self {
            ScaleKind::Major => &[0, 2, 4, 5, 7, 9, 11],
            ScaleKind::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            ScaleKind::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            ScaleKind::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            ScaleKind::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            ScaleKind::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            ScaleKind::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            ScaleKind::Locrian => &[0, 1, 3, 5, 6, 8, 10],
            ScaleKind::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            ScaleKind::MelodicMinor => &[0, 2, 3, 5, 7, 9, 11],
        }
    }
    pub const ALL: [ScaleKind; 10] = [
        ScaleKind::Major,
        ScaleKind::NaturalMinor,
        ScaleKind::Chromatic,
        ScaleKind::Dorian,
        ScaleKind::Phrygian,
        ScaleKind::Lydian,
        ScaleKind::Mixolydian,
        ScaleKind::Locrian,
        ScaleKind::HarmonicMinor,
        ScaleKind::MelodicMinor,
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
/// Octave is omitted (used where matching is octave-folded).
pub fn note_label(midi: i64) -> String {
    let (letter, _octave) = pitch::midi_name(midi);
    format!("{} ({})", SOLFEGE[midi.rem_euclid(12) as usize], letter)
}

/// Like [`note_label`] but with the octave, e.g. `note_label_oct(60)` → "Dó (C4)".
/// Used in octave-strict mode, where which octave to sing matters.
pub fn note_label_oct(midi: i64) -> String {
    let (letter, octave) = pitch::midi_name(midi);
    format!(
        "{} ({letter}{octave})",
        SOLFEGE[midi.rem_euclid(12) as usize]
    )
}

/// How fast the hold decays while out of the window, relative to how fast it
/// accumulates while in. >1 means an off-pitch stretch costs more than it gained,
/// so only a pitch that's in-window the clear majority of the time can collect.
const OUT_DECAY: f64 = 2.0;

/// Tracks how long each still-uncollected target has been sung in-tune and reports
/// targets as "collected" once held for the sustain time. When `strict` is false,
/// matching is octave-folded (only the pitch class matters); when true, the sung
/// pitch must be in the *exact* octave of the target.
pub struct Matcher {
    /// Absolute target MIDI notes (used directly in strict mode; folded otherwise).
    targets: Vec<i64>,
    strict: bool,
    held_ms: Vec<f64>,
    collected: Vec<bool>,
    cents_window: f64,
    sustain_ms: f64,
}

impl Matcher {
    pub fn new(item: &[i64], cents_window: f64, sustain_ms: f64, strict: bool) -> Self {
        let len = item.len();
        Matcher {
            targets: item.to_vec(),
            strict,
            held_ms: vec![0.0; len],
            collected: vec![false; len],
            cents_window,
            sustain_ms,
        }
    }

    /// Signed cents from `sung` to target `i`: to the exact note (strict) or to the
    /// nearest octave of its pitch class (folded).
    fn cents_to(&self, sung: f64, i: usize) -> f64 {
        if self.strict {
            (sung - self.targets[i] as f64) * 100.0
        } else {
            cents_from_class(sung, self.targets[i].rem_euclid(12))
        }
    }

    /// Feed one analysis frame. `sung` is a continuous MIDI value (note + cents/100)
    /// or `None` on silence; `dt_ms` is the time since the previous frame. Returns
    /// the indices that became collected on this frame.
    ///
    /// Holding in-window accumulates time (capped at `sustain_ms`, so excess can't
    /// be "banked"); leaving the window *decays* the hold at [`OUT_DECAY`]× rather
    /// than zeroing it, so a brief onset wobble or vibrato swing doesn't throw away
    /// near-complete progress. Because the decay is faster than the accumulation,
    /// a pitch that's out of the window more than ~1/3 of the time still falls to
    /// zero and never collects — so this forgives flutter without false positives.
    pub fn update(&mut self, sung: Option<f64>, dt_ms: f64) -> Vec<usize> {
        let mut newly = Vec::new();
        for i in 0..self.targets.len() {
            if self.collected[i] {
                continue;
            }
            let in_window = sung
                .map(|p| self.cents_to(p, i).abs() <= self.cents_window)
                .unwrap_or(false);
            if in_window {
                self.held_ms[i] = (self.held_ms[i] + dt_ms).min(self.sustain_ms);
                if self.held_ms[i] >= self.sustain_ms {
                    self.collected[i] = true;
                    newly.push(i);
                }
            } else {
                self.held_ms[i] = (self.held_ms[i] - dt_ms * OUT_DECAY).max(0.0);
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

    /// Drive realistic singing through the *actual* runtime chain
    /// (`pitch::Smoother` → `frequency_to_note` → `Matcher`) and return the ms at
    /// which the single target collects, or `None` if it never does within `secs`.
    /// `offset_cents` = steady sharp/flat error; `vibrato_cents` = ± vibrato depth.
    fn time_to_collect(
        target_midi: i64,
        sing_midi: i64,
        offset_cents: f64,
        vibrato_cents: f64,
        cents_window: f64,
        sustain_ms: f64,
        secs: f64,
    ) -> Option<f64> {
        use std::f64::consts::PI;
        let dt_ms = 33.0; // matcher tick cadence
        let base = pitch::note_to_frequency(sing_midi as f64 + offset_cents / 100.0, pitch::A4);
        let mut sm = pitch::Smoother::default();
        let mut m = Matcher::new(&[target_midi], cents_window, sustain_ms, false);
        let frames = (secs * 1000.0 / dt_ms) as usize;
        for i in 0..frames {
            let t = i as f64 * dt_ms / 1000.0;
            let vib = 2f64.powf((vibrato_cents / 1200.0) * (2.0 * PI * 5.5 * t).sin());
            let smoothed = sm.update(Some(base * vib));
            let sung = smoothed.map(|f| {
                let n = pitch::frequency_to_note(f, pitch::A4);
                n.midi + n.cents / 100.0
            });
            if !m.update(sung, dt_ms).is_empty() {
                return Some(i as f64 * dt_ms);
            }
        }
        None
    }

    // End-to-end matcher-chain behavior (default ±50¢ window, 500ms sustain).
    const W: f64 = 50.0;
    const S: f64 = 500.0;

    #[test]
    fn steady_on_pitch_collects_promptly() {
        // A clean sustained vowel should pass at roughly the sustain time.
        let ms = time_to_collect(60, 60, 0.0, 0.0, W, S, 4.0).expect("should collect");
        assert!(ms <= S + 100.0, "took {ms} ms");
    }

    #[test]
    fn vibrato_is_tolerated() {
        // The EMA smoother averages vibrato toward the center, so even wide
        // vibrato still collects (it shouldn't punish natural singing).
        for vib in [20.0, 40.0, 70.0] {
            assert!(
                time_to_collect(60, 60, 0.0, vib, W, S, 4.0).is_some(),
                "±{vib}¢ vibrato never collected"
            );
        }
    }

    #[test]
    fn octave_errors_still_match() {
        // Octave-folded matching: singing the target an octave up passes.
        assert!(time_to_collect(60, 72, 0.0, 20.0, W, S, 4.0).is_some());
    }

    #[test]
    fn clearly_off_pitch_is_rejected() {
        // 55¢ flat is past the ±50¢ window (closer to the next semitone): no pass.
        assert!(time_to_collect(60, 60, -55.0, 0.0, W, S, 4.0).is_none());
    }

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
        // Every mode / minor-variant is a 7-degree scale.
        for k in [
            ScaleKind::Dorian,
            ScaleKind::Phrygian,
            ScaleKind::Lydian,
            ScaleKind::Mixolydian,
            ScaleKind::Locrian,
            ScaleKind::HarmonicMinor,
            ScaleKind::MelodicMinor,
        ] {
            assert_eq!(k.degrees().len(), 7, "{:?}", k);
        }
    }

    #[test]
    fn mode_degrees_are_correct() {
        // Characteristic tones: Lydian #4, Mixolydian b7, Dorian (maj6 over minor),
        // harmonic minor's raised 7th, melodic minor's raised 6th+7th.
        assert_eq!(ScaleKind::Dorian.degrees(), &[0, 2, 3, 5, 7, 9, 10]);
        assert_eq!(ScaleKind::Lydian.degrees(), &[0, 2, 4, 6, 7, 9, 11]);
        assert_eq!(ScaleKind::Mixolydian.degrees(), &[0, 2, 4, 5, 7, 9, 10]);
        assert_eq!(ScaleKind::HarmonicMinor.degrees(), &[0, 2, 3, 5, 7, 8, 11]);
        assert_eq!(ScaleKind::MelodicMinor.degrees(), &[0, 2, 3, 5, 7, 9, 11]);
    }

    #[test]
    fn all_kinds_label_and_index_align() {
        // ALL is in index order, and every kind has a non-empty label.
        for (i, k) in ScaleKind::ALL.iter().enumerate() {
            assert_eq!(k.index(), i);
            assert!(!k.label().is_empty());
        }
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
        let mut m = Matcher::new(&[60], 50.0, 500.0, false);
        for _ in 0..4 {
            assert!(m.update(Some(60.0), 100.0).is_empty());
        }
        assert_eq!(m.update(Some(60.0), 100.0), vec![0]); // 500ms reached
        assert!(m.all_collected());
        // Already collected → never returned again.
        assert!(m.update(Some(60.0), 100.0).is_empty());
    }

    #[test]
    fn brief_excursion_decays_but_does_not_reset() {
        // Held 300ms, then a short 50ms dip out of window: with OUT_DECAY=2 the
        // hold drops by 100ms (to 200ms), not to zero — progress is preserved.
        let mut m = Matcher::new(&[60], 50.0, 500.0, false);
        m.update(Some(60.0), 300.0);
        m.update(Some(63.0), 50.0); // out → 300 - 50*2 = 200
        // Only ~300ms more in-window is needed (not a fresh 500ms): 200→…→500.
        for _ in 0..2 {
            assert!(m.update(Some(60.0), 100.0).is_empty()); // 300, 400
        }
        assert_eq!(m.update(Some(60.0), 100.0), vec![0]); // 500 → collect
    }

    #[test]
    fn mostly_off_pitch_never_collects() {
        // False-positive guard: alternating in/out where out dominates can never
        // reach sustain — the 2× decay outpaces the accumulation.
        let mut m = Matcher::new(&[60], 50.0, 500.0, false);
        for _ in 0..60 {
            m.update(Some(60.0), 50.0); // +50 in
            m.update(Some(63.0), 50.0); // -100 out → net -50 per pair
        }
        assert!(!m.all_collected(), "off-pitch flutter should not collect");
    }

    #[test]
    fn sustained_silence_decays_to_zero() {
        let mut m = Matcher::new(&[60], 50.0, 500.0, false);
        m.update(Some(60.0), 300.0);
        // Enough silence to fully decay (300 / (100*2) → ~2 frames), then it must
        // take a full hold again, never collecting on silence alone.
        for _ in 0..4 {
            m.update(None, 100.0);
        }
        assert!(!m.all_collected());
    }

    #[test]
    fn matching_is_octave_folded() {
        let mut m = Matcher::new(&[60], 50.0, 500.0, false); // target C
        // Singing C an octave up (72) still matches the pitch class.
        assert_eq!(m.update(Some(72.0), 500.0), vec![0]);
    }

    #[test]
    fn strict_mode_requires_exact_octave() {
        // Target C4 (60), strict: singing C5 (72) is the right class but wrong
        // octave → must NOT collect.
        let mut m = Matcher::new(&[60], 50.0, 500.0, true);
        for _ in 0..10 {
            assert!(m.update(Some(72.0), 100.0).is_empty());
        }
        assert!(!m.all_collected());
        // Singing the exact octave (C4) collects.
        let mut m = Matcher::new(&[60], 50.0, 500.0, true);
        assert_eq!(m.update(Some(60.0), 500.0), vec![0]);
    }

    #[test]
    fn note_label_oct_includes_octave() {
        assert_eq!(note_label_oct(60), "Dó (C4)");
        assert_eq!(note_label_oct(69), "Lá (A4)");
        assert_eq!(note_label_oct(48), "Dó (C3)");
    }

    #[test]
    fn chord_collects_each_target_independently() {
        let mut m = Matcher::new(&[60, 64, 67], 50.0, 100.0, false); // C E G
        assert_eq!(m.update(Some(64.0), 100.0), vec![1]); // sing E
        assert_eq!(m.update(Some(60.0), 100.0), vec![0]); // sing C
        assert!(!m.all_collected());
        assert_eq!(m.update(Some(67.0), 100.0), vec![2]); // sing G
        assert!(m.all_collected());
    }
}
