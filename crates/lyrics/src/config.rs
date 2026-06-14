//! Persisted lyrics settings (font size + enabled), stored as JSON via `overlay`.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct SavedConfig {
    #[serde(default = "default_font_idx")]
    pub font_size_idx: usize,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_font_idx() -> usize {
    1
}
fn default_enabled() -> bool {
    true
}

impl Default for SavedConfig {
    fn default() -> Self {
        SavedConfig {
            font_size_idx: 1,
            enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_config_roundtrips_json() {
        let cfg = SavedConfig {
            font_size_idx: 2,
            enabled: false,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: SavedConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.font_size_idx, 2);
        assert!(!loaded.enabled);
    }

    #[test]
    fn saved_config_missing_fields_use_defaults() {
        let cfg: SavedConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.font_size_idx, 1);
        assert!(cfg.enabled);
    }

    #[test]
    fn saved_config_ignores_legacy_mode_idx() {
        let cfg: SavedConfig =
            serde_json::from_str(r#"{"font_size_idx":2,"mode_idx":1,"enabled":true}"#).unwrap();
        assert_eq!(cfg.font_size_idx, 2);
        assert!(cfg.enabled);
    }
}
