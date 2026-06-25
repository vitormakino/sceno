//! Persisted metronome settings, stored as JSON via `overlay`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Where the tempo comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    /// Set by hand (tray ± and tap tempo).
    #[default]
    Manual,
    /// Locked to the playing track's UltraStar `#BPM`/`#GAP` grid.
    Song,
    /// Estimated live from the system-audio monitor (best-effort).
    Detect,
}

impl Source {
    pub fn index(self) -> usize {
        match self {
            Source::Manual => 0,
            Source::Song => 1,
            Source::Detect => 2,
        }
    }

    pub fn from_idx(i: usize) -> Self {
        match i {
            1 => Source::Song,
            2 => Source::Detect,
            _ => Source::Manual,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Source::Manual => "Manual",
            Source::Song => "Seguir música",
            Source::Detect => "Detectar áudio",
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct MetronomeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub running: bool,
    #[serde(default = "default_bpm")]
    pub bpm: f64,
    #[serde(default = "default_beats_per_bar")]
    pub beats_per_bar: u32,
    #[serde(default)]
    pub source_idx: usize,
    #[serde(default = "default_true")]
    pub audible: bool,
    #[serde(default = "default_true")]
    pub flash: bool,
    /// Per-song phase nudge in ms, keyed by `TrackQuery::key()` (Song source).
    #[serde(default)]
    pub offsets: HashMap<String, i64>,
}

fn default_true() -> bool {
    true
}
fn default_bpm() -> f64 {
    120.0
}
fn default_beats_per_bar() -> u32 {
    4
}

impl Default for MetronomeConfig {
    fn default() -> Self {
        MetronomeConfig {
            enabled: true,
            running: false,
            bpm: 120.0,
            beats_per_bar: 4,
            source_idx: 0,
            audible: true,
            flash: true,
            offsets: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_index_roundtrips() {
        for s in [Source::Manual, Source::Song, Source::Detect] {
            assert_eq!(Source::from_idx(s.index()), s);
        }
        assert_eq!(Source::from_idx(99), Source::Manual);
    }

    #[test]
    fn roundtrips_json() {
        let mut offsets = HashMap::new();
        offsets.insert("artist|title|".to_string(), -200);
        let cfg = MetronomeConfig {
            enabled: false,
            running: true,
            bpm: 96.0,
            beats_per_bar: 3,
            source_idx: 2,
            audible: false,
            flash: true,
            offsets,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: MetronomeConfig = serde_json::from_str(&json).unwrap();
        assert!(!loaded.enabled);
        assert!(loaded.running);
        assert_eq!(loaded.bpm, 96.0);
        assert_eq!(loaded.beats_per_bar, 3);
        assert_eq!(loaded.source_idx, 2);
        assert!(!loaded.audible);
        assert_eq!(loaded.offsets.get("artist|title|"), Some(&-200));
    }

    #[test]
    fn missing_fields_use_defaults() {
        let cfg: MetronomeConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.enabled);
        assert!(!cfg.running);
        assert_eq!(cfg.bpm, 120.0);
        assert_eq!(cfg.beats_per_bar, 4);
        assert!(cfg.audible);
        assert!(cfg.flash);
    }
}
