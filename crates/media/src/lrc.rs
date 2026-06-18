//! Parsing of LRC synced-lyrics strings into the source-agnostic `CueEntry` timeline.
//!
//! Supports standard `[mm:ss.xx]` timestamps, multiple timestamps per line
//! (`[00:12][00:50]repeated line`) and enhanced word-level tags (`<00:12.3>`).
//! Word tags are stripped from [`CueEntry::text`] but their onsets are preserved
//! in [`CueEntry::words`] (only for single-timestamp lines — a repeated line's
//! absolute word times would not line up at its later occurrences). Metadata tags
//! such as `[ar:...]` / `[length:...]` are ignored. Blank lyric lines are kept as
//! empty-text cues so the overlay clears during instrumental breaks.

use crate::cue::{CueEntry, WordTiming};

/// How long the final cue stays on screen (no following line to bound it).
const LAST_CUE_SECS: f64 = 5.0;

/// Parse an LRC string into time-ordered cues. Cue `end` is the next cue's
/// `start`; the last cue ends `LAST_CUE_SECS` after its start.
pub fn parse_lrc(input: &str) -> Vec<CueEntry> {
    let mut entries: Vec<(f64, String, Vec<WordTiming>)> = Vec::new();

    for line in input.lines() {
        let mut rest = line.trim_start();
        let mut times: Vec<f64> = Vec::new();

        // Consume leading `[..]` tags. Timestamps are collected; the first
        // non-timestamp tag ends the prefix (metadata-only lines yield none).
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(close) = stripped.find(']') else {
                break;
            };
            match parse_time(&stripped[..close]) {
                Some(t) => {
                    times.push(t);
                    rest = stripped[close + 1..].trim_start();
                }
                None => break,
            }
        }

        if times.is_empty() {
            continue;
        }

        let text = strip_word_tags(rest).trim().to_string();
        // Word onsets are only meaningful when the line plays once: a repeated
        // line ([t1][t2]…) shares one set of absolute word times that cannot be
        // correct at every occurrence, so those fall back to line-level.
        let words = if times.len() == 1 {
            parse_word_timings(rest, times[0])
        } else {
            Vec::new()
        };
        for (i, t) in times.iter().enumerate() {
            let w = if i == 0 { words.clone() } else { Vec::new() };
            entries.push((*t, text.clone(), w));
        }
    }

    entries.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let ends: Vec<f64> = (0..entries.len())
        .map(|i| {
            entries
                .get(i + 1)
                .map_or(entries[i].0 + LAST_CUE_SECS, |n| n.0)
        })
        .collect();

    entries
        .into_iter()
        .zip(ends)
        .map(|((start, text, words), end)| CueEntry {
            start,
            end,
            text,
            words,
        })
        .collect()
}

/// Parse a `mm:ss`, `mm:ss.xx` or `mm:ss.xxx` timestamp into seconds.
/// Returns `None` for non-timestamp tag contents (e.g. `ar:Artist`).
fn parse_time(s: &str) -> Option<f64> {
    let (mm, ss) = s.split_once(':')?;
    let minutes: f64 = mm.trim().parse().ok()?;
    let seconds: f64 = ss.trim().parse().ok()?;
    if minutes < 0.0 || seconds < 0.0 {
        return None;
    }
    Some(minutes * 60.0 + seconds)
}

/// Parse enhanced word-level tags (`<mm:ss.xx>word`) into per-word onsets. Text
/// before the first tag attaches to `line_start`. Returns empty when the line has
/// no time tags (line-level lyric), so callers render the whole line at once. Each
/// word keeps its internal spacing, so concatenating all `text` reproduces the line.
fn parse_word_timings(s: &str, line_start: f64) -> Vec<WordTiming> {
    let mut words: Vec<WordTiming> = Vec::new();
    let mut buf = String::new();
    let mut cur: Option<f64> = None;
    let mut saw_tag = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '<' {
            buf.push(c);
            continue;
        }
        // Read up to the matching '>'.
        let mut tag = String::new();
        let mut closed = false;
        for d in chars.by_ref() {
            if d == '>' {
                closed = true;
                break;
            }
            tag.push(d);
        }
        match (closed, parse_time(&tag)) {
            (true, Some(t)) => {
                if !buf.is_empty() {
                    words.push(WordTiming {
                        start: cur.unwrap_or(line_start),
                        text: std::mem::take(&mut buf),
                    });
                }
                cur = Some(t);
                saw_tag = true;
            }
            // Not a time tag: keep the literal text (rare; e.g. stray '<').
            _ => {
                buf.push('<');
                buf.push_str(&tag);
                if closed {
                    buf.push('>');
                }
            }
        }
    }

    if !saw_tag {
        return Vec::new();
    }
    if !buf.is_empty() {
        words.push(WordTiming {
            start: cur.unwrap_or(line_start),
            text: buf,
        });
    }
    words
}

/// Remove enhanced word-level tags like `<00:12.34>` from a lyric line.
fn strip_word_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0u32;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_lines() {
        let lrc = "[00:12.34]hello\n[00:15.00]world";
        let cues = parse_lrc(lrc);
        assert_eq!(cues.len(), 2);
        assert!((cues[0].start - 12.34).abs() < 1e-6);
        assert_eq!(cues[0].text, "hello");
        // end bounded by next start
        assert!((cues[0].end - 15.0).abs() < 1e-6);
        assert_eq!(cues[1].text, "world");
        // last cue gets a default tail
        assert!((cues[1].end - (15.0 + LAST_CUE_SECS)).abs() < 1e-6);
    }

    #[test]
    fn ignores_metadata_tags() {
        let lrc = "[ar:Some Artist]\n[ti:Song]\n[length:03:43]\n[00:01.00]line";
        let cues = parse_lrc(lrc);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "line");
        assert!((cues[0].start - 1.0).abs() < 1e-6);
    }

    #[test]
    fn handles_multi_digit_minutes() {
        let cues = parse_lrc("[10:05.50]late line");
        assert_eq!(cues.len(), 1);
        assert!((cues[0].start - 605.5).abs() < 1e-6);
    }

    #[test]
    fn expands_multiple_timestamps_per_line() {
        let cues = parse_lrc("[00:10.00][00:40.00]chorus");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "chorus");
        assert_eq!(cues[1].text, "chorus");
        assert!((cues[0].start - 10.0).abs() < 1e-6);
        assert!((cues[1].start - 40.0).abs() < 1e-6);
        // first cue is bounded by the second occurrence
        assert!((cues[0].end - 40.0).abs() < 1e-6);
    }

    #[test]
    fn strips_word_level_tags() {
        let cues = parse_lrc("[00:01.00]<00:01.00>hello <00:01.50>world");
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "hello world");
    }

    #[test]
    fn parses_word_level_timings() {
        let cues = parse_lrc("[00:01.00]<00:01.00>hello <00:01.50>world");
        assert_eq!(cues.len(), 1);
        let w = &cues[0].words;
        assert_eq!(w.len(), 2);
        assert!((w[0].start - 1.0).abs() < 1e-6);
        assert_eq!(w[0].text, "hello ");
        assert!((w[1].start - 1.5).abs() < 1e-6);
        assert_eq!(w[1].text, "world");
        // Joining the words reproduces the line.
        let joined: String = w.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(joined, "hello world");
    }

    #[test]
    fn leading_text_before_first_word_tag_uses_line_start() {
        let cues = parse_lrc("[00:02.00]oh <00:03.00>yeah");
        let w = &cues[0].words;
        assert_eq!(w.len(), 2);
        assert!((w[0].start - 2.0).abs() < 1e-6); // herda o início da linha
        assert_eq!(w[0].text, "oh ");
        assert!((w[1].start - 3.0).abs() < 1e-6);
    }

    #[test]
    fn plain_line_has_no_word_timings() {
        let cues = parse_lrc("[00:01.00]just a line");
        assert_eq!(cues[0].text, "just a line");
        assert!(cues[0].words.is_empty());
    }

    #[test]
    fn repeated_line_falls_back_to_line_level() {
        // Multiple timestamps: word onsets can't be right at both, so none kept.
        let cues = parse_lrc("[00:10.00][00:40.00]<00:10.00>la <00:10.50>la");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "la la");
        assert!(cues[0].words.is_empty());
        assert!(cues[1].words.is_empty());
    }

    #[test]
    fn keeps_blank_line_as_empty_cue() {
        // instrumental gap: empty text clears the overlay
        let cues = parse_lrc("[00:01.00]sing\n[00:05.00]\n[00:08.00]more");
        assert_eq!(cues.len(), 3);
        assert_eq!(cues[1].text, "");
        assert!((cues[1].start - 5.0).abs() < 1e-6);
    }

    #[test]
    fn sorts_out_of_order_timestamps() {
        let cues = parse_lrc("[00:20.00]second\n[00:10.00]first");
        assert_eq!(cues[0].text, "first");
        assert_eq!(cues[1].text, "second");
    }

    #[test]
    fn empty_input_yields_no_cues() {
        assert!(parse_lrc("").is_empty());
        assert!(parse_lrc("\n\n   \n").is_empty());
    }

    #[test]
    fn parses_seconds_without_fraction() {
        let cues = parse_lrc("[01:02]plain");
        assert_eq!(cues.len(), 1);
        assert!((cues[0].start - 62.0).abs() < 1e-6);
    }
}
