//! Pure pitch → note math (no audio, no UI). Fully unit-tested.

/// A detected note: name, octave, and cents deviation from perfect pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub name: &'static str,
    pub octave: i32,
    pub cents: f64,
}
