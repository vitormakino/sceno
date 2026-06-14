//! Persisted tuner settings (meter style + enabled), stored as JSON via `overlay`.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct TunerConfig {
    #[serde(default)]
    pub meter_style_idx: usize,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Default for TunerConfig {
    fn default() -> Self {
        TunerConfig {
            meter_style_idx: 0,
            enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_json() {
        let cfg = TunerConfig {
            meter_style_idx: 2,
            enabled: false,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: TunerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.meter_style_idx, 2);
        assert!(!loaded.enabled);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let cfg: TunerConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.meter_style_idx, 0);
        assert!(cfg.enabled);
    }
}
