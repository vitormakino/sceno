//! The source-agnostic timed-lyrics model shared by every lyrics source.

/// One timed lyric line: visible on `[start, end)` (seconds).
#[derive(Debug, Clone)]
pub struct CueEntry {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// The text of the cue whose `[start, end)` interval contains `t`, if any.
pub fn cue_at(cues: &[CueEntry], t: f64) -> Option<&str> {
    cues.iter()
        .find(|c| c.start <= t && t < c.end)
        .map(|c| c.text.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cues() -> Vec<CueEntry> {
        vec![
            CueEntry {
                start: 1.0,
                end: 3.0,
                text: "hello".into(),
            },
            CueEntry {
                start: 3.0,
                end: 5.0,
                text: "world".into(),
            },
            CueEntry {
                start: 5.0,
                end: 7.0,
                text: "foo".into(),
            },
        ]
    }

    #[test]
    fn cue_at_returns_active_cue() {
        let cues = sample_cues();
        assert_eq!(cue_at(&cues, 1.0), Some("hello"));
        assert_eq!(cue_at(&cues, 2.9), Some("hello"));
        assert_eq!(cue_at(&cues, 3.0), Some("world")); // start é inclusivo
        assert_eq!(cue_at(&cues, 4.5), Some("world"));
        assert_eq!(cue_at(&cues, 6.0), Some("foo"));
    }

    #[test]
    fn cue_at_none_outside_cues() {
        let cues = vec![
            CueEntry {
                start: 1.0,
                end: 2.0,
                text: "a".into(),
            },
            CueEntry {
                start: 3.0,
                end: 4.0,
                text: "b".into(),
            },
        ];
        assert_eq!(cue_at(&cues, 0.5), None);
        assert_eq!(cue_at(&cues, 2.0), None); // end é exclusivo
        assert_eq!(cue_at(&cues, 2.5), None); // gap entre cues
        assert_eq!(cue_at(&cues, 4.0), None);
    }

    #[test]
    fn cue_at_empty_list() {
        assert_eq!(cue_at(&[], 1.0), None);
    }
}
