//! Per-note practice statistics: how long, on average, you take to nail each
//! pitch class. Stored separately from the settings config (in the app data dir)
//! so "Restaurar padrões" never wipes practice history.

use serde::{Deserialize, Serialize};

/// App name for the stats file path (kept in sync with `crate::APP`).
const APP: &str = "vocalize";
/// Minimum samples before a pitch class is eligible to be the "hardest" — avoids
/// a single slow attempt dominating the readout.
const MIN_SAMPLES: u32 = 3;

/// Accumulated time-to-collect for one pitch class.
#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Debug)]
pub struct Entry {
    pub count: u32,
    pub total_ms: f64,
}

/// Time-to-sing stats indexed by pitch class (0 = C … 11 = B).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Stats {
    /// Always length 12 (one entry per pitch class).
    by_class: Vec<Entry>,
}

impl Default for Stats {
    fn default() -> Self {
        Stats {
            by_class: vec![Entry::default(); 12],
        }
    }
}

impl Stats {
    /// Record that `class` was sung correctly after `ms` from the item being armed.
    pub fn record(&mut self, class: i64, ms: f64) {
        if self.by_class.len() < 12 {
            self.by_class.resize(12, Entry::default());
        }
        let e = &mut self.by_class[class.rem_euclid(12) as usize];
        e.count += 1;
        e.total_ms += ms;
    }

    /// Average time-to-sing (ms) for a pitch class, or `None` if never sung.
    pub fn avg_ms(&self, class: usize) -> Option<f64> {
        let e = self.by_class.get(class)?;
        (e.count > 0).then(|| e.total_ms / e.count as f64)
    }

    /// The pitch class you take longest to nail, among those with at least
    /// [`MIN_SAMPLES`] samples, as `(class, avg_ms)`.
    pub fn hardest(&self) -> Option<(usize, f64)> {
        (0..12)
            .filter_map(|c| {
                let e = self.by_class.get(c)?;
                if e.count < MIN_SAMPLES {
                    return None;
                }
                Some((c, self.avg_ms(c)?))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Discard all history.
    pub fn clear(&mut self) {
        self.by_class = vec![Entry::default(); 12];
    }
}

/// Path to the stats file (`<data_dir>/vocalize/stats.json`).
fn stats_path() -> Option<std::path::PathBuf> {
    overlay::data_dir(APP).map(|d| d.join("stats.json"))
}

/// Load saved stats, or defaults on first run / malformed file.
pub fn load() -> Stats {
    stats_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist stats (best-effort; silently ignores I/O errors).
pub fn save(stats: &Stats) {
    let Some(path) = stats_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(stats) {
        let _ = std::fs::write(&path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_average() {
        let mut s = Stats::default();
        s.record(0, 1000.0);
        s.record(0, 2000.0);
        assert_eq!(s.avg_ms(0), Some(1500.0));
        assert_eq!(s.avg_ms(1), None);
    }

    #[test]
    fn octave_folds_to_pitch_class() {
        let mut s = Stats::default();
        s.record(60, 500.0); // C4 → class 0
        s.record(72, 700.0); // C5 → class 0
        assert_eq!(s.avg_ms(0), Some(600.0));
    }

    #[test]
    fn hardest_needs_min_samples_and_picks_slowest() {
        let mut s = Stats::default();
        // class 2 is slower on average but...
        s.record(2, 3000.0);
        s.record(2, 3000.0); // only 2 samples → below MIN_SAMPLES
        // class 5 has enough samples
        for _ in 0..3 {
            s.record(5, 1000.0);
        }
        assert_eq!(s.hardest(), Some((5, 1000.0)));
        // Once class 2 reaches the threshold, it wins (slower).
        s.record(2, 3000.0);
        assert_eq!(s.hardest(), Some((2, 3000.0)));
    }

    #[test]
    fn clear_resets() {
        let mut s = Stats::default();
        s.record(0, 1000.0);
        s.clear();
        assert_eq!(s.avg_ms(0), None);
        assert!(s.hardest().is_none());
    }

    #[test]
    fn roundtrips_json() {
        let mut s = Stats::default();
        s.record(7, 1234.0);
        let json = serde_json::to_string(&s).unwrap();
        let back: Stats = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
