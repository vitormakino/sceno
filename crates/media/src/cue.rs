//! The source-agnostic timed-lyrics model shared by every lyrics source.

/// Onset of one word within a line, in the same time scale as [`CueEntry::start`]
/// (seconds). `text` keeps its trailing space, so joining all words reproduces the
/// line verbatim.
#[derive(Debug, Clone)]
pub struct WordTiming {
    pub start: f64,
    pub text: String,
}

/// One timed lyric line: visible on `[start, end)` (seconds). `words` carries
/// per-word onsets for sources that have them (enhanced LRC); it is empty when the
/// source is line-level only, in which case consumers render the whole line at once.
#[derive(Debug, Clone)]
pub struct CueEntry {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<WordTiming>,
}

impl CueEntry {
    /// How many leading words have been reached at time `t` (`0..=words.len()`).
    /// Returns `0` when there are no word timings (the caller renders the whole line).
    pub fn sung_words(&self, t: f64) -> usize {
        self.words.iter().take_while(|w| t >= w.start).count()
    }
}

/// The active line at `t` and the next non-empty line after it, for lookahead.
#[derive(Debug, Default)]
pub struct ActiveLines<'a> {
    pub current: Option<&'a CueEntry>,
    pub next: Option<&'a CueEntry>,
}

/// The text of the cue whose `[start, end)` interval contains `t`, if any.
pub fn cue_at(cues: &[CueEntry], t: f64) -> Option<&str> {
    cues.iter()
        .find(|c| c.start <= t && t < c.end)
        .map(|c| c.text.as_str())
}

/// The active cue at `t` plus the next cue with non-empty text (skipping
/// instrumental-gap markers), so the overlay can show an upcoming line dimmed.
pub fn lines_at(cues: &[CueEntry], t: f64) -> ActiveLines<'_> {
    let current_idx = cues.iter().position(|c| c.start <= t && t < c.end);
    let next = match current_idx {
        Some(i) => cues[i + 1..].iter().find(|c| !c.text.is_empty()),
        // No active line (gap/intro): look ahead from the next cue to start.
        None => cues.iter().find(|c| c.start > t && !c.text.is_empty()),
    };
    ActiveLines {
        current: current_idx.map(|i| &cues[i]),
        next,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(start: f64, end: f64, text: &str) -> CueEntry {
        CueEntry {
            start,
            end,
            text: text.into(),
            words: Vec::new(),
        }
    }

    fn sample_cues() -> Vec<CueEntry> {
        vec![
            cue(1.0, 3.0, "hello"),
            cue(3.0, 5.0, "world"),
            cue(5.0, 7.0, "foo"),
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
        let cues = vec![cue(1.0, 2.0, "a"), cue(3.0, 4.0, "b")];
        assert_eq!(cue_at(&cues, 0.5), None);
        assert_eq!(cue_at(&cues, 2.0), None); // end é exclusivo
        assert_eq!(cue_at(&cues, 2.5), None); // gap entre cues
        assert_eq!(cue_at(&cues, 4.0), None);
    }

    #[test]
    fn cue_at_empty_list() {
        assert_eq!(cue_at(&[], 1.0), None);
    }

    fn word(start: f64, text: &str) -> WordTiming {
        WordTiming {
            start,
            text: text.into(),
        }
    }

    #[test]
    fn sung_words_counts_reached_onsets() {
        let c = CueEntry {
            start: 1.0,
            end: 3.0,
            text: "hello world".into(),
            words: vec![word(1.0, "hello "), word(1.5, "world")],
        };
        assert_eq!(c.sung_words(0.9), 0); // antes da primeira palavra
        assert_eq!(c.sung_words(1.0), 1); // onset é inclusivo
        assert_eq!(c.sung_words(1.4), 1);
        assert_eq!(c.sung_words(1.5), 2);
        assert_eq!(c.sung_words(9.0), 2); // saturado no total
    }

    #[test]
    fn sung_words_zero_without_word_timings() {
        let c = cue(1.0, 3.0, "plain line");
        assert_eq!(c.sung_words(2.0), 0);
    }

    #[test]
    fn lines_at_gives_current_and_next() {
        let cues = sample_cues();
        let l = lines_at(&cues, 2.0);
        assert_eq!(l.current.map(|c| c.text.as_str()), Some("hello"));
        assert_eq!(l.next.map(|c| c.text.as_str()), Some("world"));
    }

    #[test]
    fn lines_at_next_skips_empty_cues() {
        // "world" is an instrumental gap (empty text); lookahead jumps to "foo".
        let cues = vec![
            cue(1.0, 3.0, "hello"),
            cue(3.0, 5.0, ""),
            cue(5.0, 7.0, "foo"),
        ];
        let l = lines_at(&cues, 2.0);
        assert_eq!(l.current.map(|c| c.text.as_str()), Some("hello"));
        assert_eq!(l.next.map(|c| c.text.as_str()), Some("foo"));
    }

    #[test]
    fn lines_at_in_gap_has_no_current_but_finds_next() {
        let cues = vec![cue(1.0, 2.0, "a"), cue(3.0, 4.0, "b")];
        let l = lines_at(&cues, 2.5); // no gap entre cues
        assert!(l.current.is_none());
        assert_eq!(l.next.map(|c| c.text.as_str()), Some("b"));
    }

    #[test]
    fn lines_at_past_end_is_empty() {
        let cues = sample_cues();
        let l = lines_at(&cues, 100.0);
        assert!(l.current.is_none());
        assert!(l.next.is_none());
    }

    #[test]
    fn lines_at_empty_list() {
        let l = lines_at(&[], 1.0);
        assert!(l.current.is_none());
        assert!(l.next.is_none());
    }
}
