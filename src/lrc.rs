//! Parsing of LRC synced-lyrics strings into the app's `CueEntry` timeline.
//!
//! Supports standard `[mm:ss.xx]` timestamps, multiple timestamps per line
//! (`[00:12][00:50]repeated line`) and enhanced word-level tags (`<00:12.3>`),
//! which are stripped. Metadata tags such as `[ar:...]` / `[length:...]` are
//! ignored. Blank lyric lines are kept as empty-text cues so the overlay clears
//! during instrumental breaks.

use crate::CueEntry;

/// How long the final cue stays on screen (no following line to bound it).
const LAST_CUE_SECS: f64 = 5.0;

/// Parse an LRC string into time-ordered cues. Cue `end` is the next cue's
/// `start`; the last cue ends `LAST_CUE_SECS` after its start.
pub fn parse_lrc(input: &str) -> Vec<CueEntry> {
    let mut entries: Vec<(f64, String)> = Vec::new();

    for line in input.lines() {
        let mut rest = line.trim_start();
        let mut times: Vec<f64> = Vec::new();

        // Consume leading `[..]` tags. Timestamps are collected; the first
        // non-timestamp tag ends the prefix (metadata-only lines yield none).
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(close) = stripped.find(']') else { break };
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
        for t in times {
            entries.push((t, text.clone()));
        }
    }

    entries.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    entries
        .iter()
        .enumerate()
        .map(|(i, (start, text))| {
            let end = entries.get(i + 1).map_or(start + LAST_CUE_SECS, |n| n.0);
            CueEntry { start: *start, end, text: text.clone() }
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
