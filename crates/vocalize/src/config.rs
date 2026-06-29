//! Persisted vocalize settings, stored as JSON via `overlay`.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct VocalizeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Whether the reference tone is played aloud.
    #[serde(default = "default_true")]
    pub audible: bool,
    /// Scale root as a MIDI pitch class (0 = C … 11 = B).
    #[serde(default)]
    pub scale_root: i64,
    /// Scale kind index; see [`crate::exercise::ScaleKind`].
    #[serde(default)]
    pub scale_kind_idx: usize,
    /// Exercise mode index; see [`crate::exercise::Mode`].
    #[serde(default)]
    pub mode_idx: usize,
    /// Chord playback style index (0 = together); see [`crate::exercise::PlayStyle`].
    #[serde(default)]
    pub play_style_idx: usize,
    /// Reference-tone timbre index (0 = electric piano); see [`crate::tone::Timbre`].
    #[serde(default)]
    pub timbre_idx: usize,
    /// Half-width of the in-tune window, in cents.
    #[serde(default = "default_cents")]
    pub cents_window: f64,
    /// How long the pitch must be held in-window to count, in ms.
    #[serde(default = "default_sustain")]
    pub sustain_ms: u64,
    /// Require the exact octave (vs. octave-folded pitch-class matching).
    #[serde(default = "default_true")]
    pub octave_strict: bool,
}

fn default_true() -> bool {
    true
}
fn default_cents() -> f64 {
    50.0
}
fn default_sustain() -> u64 {
    500
}

impl Default for VocalizeConfig {
    fn default() -> Self {
        VocalizeConfig {
            enabled: true,
            audible: true,
            scale_root: 0,
            scale_kind_idx: 0,
            mode_idx: 0,
            play_style_idx: 0,
            timbre_idx: 0,
            cents_window: 50.0,
            sustain_ms: 500,
            octave_strict: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_json() {
        let cfg = VocalizeConfig {
            enabled: false,
            audible: false,
            scale_root: 9,
            scale_kind_idx: 1,
            mode_idx: 2,
            play_style_idx: 1,
            timbre_idx: 1,
            cents_window: 25.0,
            sustain_ms: 800,
            octave_strict: false,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: VocalizeConfig = serde_json::from_str(&json).unwrap();
        assert!(!loaded.enabled);
        assert!(!loaded.octave_strict);
        assert!(!loaded.audible);
        assert_eq!(loaded.scale_root, 9);
        assert_eq!(loaded.scale_kind_idx, 1);
        assert_eq!(loaded.mode_idx, 2);
        assert_eq!(loaded.play_style_idx, 1);
        assert_eq!(loaded.timbre_idx, 1);
        assert_eq!(loaded.cents_window, 25.0);
        assert_eq!(loaded.sustain_ms, 800);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let cfg: VocalizeConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.enabled);
        assert!(cfg.audible);
        assert_eq!(cfg.scale_root, 0);
        assert_eq!(cfg.mode_idx, 0);
        assert_eq!(cfg.play_style_idx, 0);
        assert_eq!(cfg.timbre_idx, 0);
        assert_eq!(cfg.cents_window, 50.0);
        assert_eq!(cfg.sustain_ms, 500);
        assert!(cfg.octave_strict);
    }
}
