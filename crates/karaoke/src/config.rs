//! Persisted karaoke settings, stored as JSON via `overlay`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
pub struct KaraokeConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// UltraStar `.txt` library directory; `None` uses `overlay::data_dir("karaoke")`.
    #[serde(default)]
    pub library_dir: Option<PathBuf>,
    /// Manual sync nudge (ms) added to the playback position, to correct drift
    /// when the playing recording's intro differs from the UltraStar reference.
    #[serde(default)]
    pub offset_ms: f64,
    /// Best accuracy score (%) per song, keyed by [`media::TrackQuery::key`].
    #[serde(default)]
    pub best_scores: HashMap<String, f64>,
}

fn default_enabled() -> bool {
    true
}

impl Default for KaraokeConfig {
    fn default() -> Self {
        KaraokeConfig {
            enabled: true,
            library_dir: None,
            offset_ms: 0.0,
            best_scores: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_json() {
        let mut best_scores = HashMap::new();
        best_scores.insert("artist|title||0".to_string(), 87.5);
        let cfg = KaraokeConfig {
            enabled: false,
            library_dir: Some(PathBuf::from("/songs")),
            offset_ms: -150.0,
            best_scores,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: KaraokeConfig = serde_json::from_str(&json).unwrap();
        assert!(!loaded.enabled);
        assert_eq!(loaded.library_dir, Some(PathBuf::from("/songs")));
        assert_eq!(loaded.offset_ms, -150.0);
        assert_eq!(loaded.best_scores.get("artist|title||0"), Some(&87.5));
    }

    #[test]
    fn missing_fields_use_defaults() {
        let cfg: KaraokeConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.library_dir, None);
        assert_eq!(cfg.offset_ms, 0.0);
        assert!(cfg.best_scores.is_empty());
    }
}
