//! Persisted tuner settings (meter style, enabled, reference pitch, instrument),
//! stored as JSON via `overlay`.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct TunerConfig {
    #[serde(default)]
    pub meter_style_idx: usize,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Concert-pitch reference for A4, in Hz (e.g. 432 / 440 / 442 / 443).
    #[serde(default = "default_a4")]
    pub a4_hz: f64,
    /// Instrument preset index (0 = chromatic); see [`crate::instrument::Instrument`].
    #[serde(default)]
    pub instrument_idx: usize,
}

fn default_enabled() -> bool {
    true
}
fn default_a4() -> f64 {
    440.0
}

impl Default for TunerConfig {
    fn default() -> Self {
        TunerConfig {
            meter_style_idx: 0,
            enabled: true,
            a4_hz: 440.0,
            instrument_idx: 0,
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
            a4_hz: 442.0,
            instrument_idx: 1,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: TunerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.meter_style_idx, 2);
        assert!(!loaded.enabled);
        assert_eq!(loaded.a4_hz, 442.0);
        assert_eq!(loaded.instrument_idx, 1);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let cfg: TunerConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.meter_style_idx, 0);
        assert!(cfg.enabled);
        assert_eq!(cfg.a4_hz, 440.0);
        assert_eq!(cfg.instrument_idx, 0);
    }
}
