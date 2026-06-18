//! Persisted lyrics settings (font size, enabled, per-song sync offsets),
//! stored as JSON via `overlay`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
pub struct SavedConfig {
    #[serde(default = "default_font_idx")]
    pub font_size_idx: usize,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Manual sync nudge (ms) per song, keyed by [`media::TrackQuery::key`].
    /// Added to the playback position so lyrics can be advanced (positive) or
    /// delayed (negative) to match an arbitrary recording. A `0` offset is not
    /// stored (the entry is removed), so this map only holds real customizations.
    #[serde(default)]
    pub offsets: HashMap<String, f64>,
    /// Whether to show the upcoming line dimmed below the active one (lookahead).
    #[serde(default = "default_show_next")]
    pub show_next: bool,
}

fn default_font_idx() -> usize {
    1
}
fn default_enabled() -> bool {
    true
}
fn default_show_next() -> bool {
    true
}

impl Default for SavedConfig {
    fn default() -> Self {
        SavedConfig {
            font_size_idx: 1,
            enabled: true,
            offsets: HashMap::new(),
            show_next: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_config_roundtrips_json() {
        let mut offsets = HashMap::new();
        offsets.insert("artist|title||0".to_string(), -150.0);
        let cfg = SavedConfig {
            font_size_idx: 2,
            enabled: false,
            offsets,
            show_next: false,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: SavedConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.font_size_idx, 2);
        assert!(!loaded.enabled);
        assert_eq!(loaded.offsets.get("artist|title||0"), Some(&-150.0));
        assert!(!loaded.show_next);
    }

    #[test]
    fn saved_config_missing_fields_use_defaults() {
        let cfg: SavedConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.font_size_idx, 1);
        assert!(cfg.enabled);
        assert!(cfg.offsets.is_empty());
        assert!(cfg.show_next);
    }

    #[test]
    fn saved_config_ignores_legacy_mode_idx() {
        let cfg: SavedConfig =
            serde_json::from_str(r#"{"font_size_idx":2,"mode_idx":1,"enabled":true}"#).unwrap();
        assert_eq!(cfg.font_size_idx, 2);
        assert!(cfg.enabled);
        assert!(cfg.offsets.is_empty());
    }
}
