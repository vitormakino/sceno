//! Parsing of UltraStar `.txt` karaoke files into timed, pitched note events.
//!
//! Format reference: headers `#TITLE/#ARTIST/#BPM/#GAP`, then note lines
//! `<type> <startBeat> <length> <pitch> <syllable>` where type is `:` (normal),
//! `*` (golden), `F` (freestyle) or `R`/`G` (rap); `-` is a line break and `E`
//! ends the song. Pitch is in half-steps relative to C4, so `midi = pitch + 60`.
//! `#BPM` is the real BPM with beats internally quadrupled, so one beat lasts
//! `60000 / (bpm * 4)` ms; a beat's absolute time is `#GAP` (ms) plus that.
//!
//! `#RELATIVE:YES` files (rare, beat offsets relative to the previous line) are
//! out of scope and rejected.

/// One sung note: visible/active on `[start, end)` seconds, at `midi` pitch.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteEvent {
    pub start: f64,
    pub end: f64,
    /// MIDI note number (60 = C4).
    pub midi: f64,
    pub text: String,
    pub golden: bool,
}

/// A parsed UltraStar song: metadata + the flat note timeline + line-break times.
#[derive(Debug, Clone)]
pub struct UltraStarSong {
    pub artist: String,
    pub title: String,
    pub gap_ms: f64,
    pub bpm: f64,
    pub notes: Vec<NoteEvent>,
    /// Absolute seconds at which each `-` line break occurs (for phrase grouping).
    pub breaks: Vec<f64>,
}

/// Parse an UltraStar `.txt` string. Returns `None` if there is no usable BPM,
/// no notes, or the file uses unsupported `#RELATIVE` mode.
pub fn parse_ultrastar(input: &str) -> Option<UltraStarSong> {
    let mut artist = String::new();
    let mut title = String::new();
    let mut bpm: Option<f64> = None;
    let mut gap_ms = 0.0;

    let mut notes: Vec<NoteEvent> = Vec::new();
    let mut breaks: Vec<f64> = Vec::new();

    // Beat → seconds, resolved once BPM is known. Headers precede notes in valid
    // files, so we read them first in a pre-pass over the header lines.
    for raw in input.lines() {
        let line = raw.trim_end();
        let Some(rest) = line.strip_prefix('#') else {
            continue;
        };
        let Some((key, value)) = rest.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_uppercase().as_str() {
            "ARTIST" => artist = value.to_string(),
            "TITLE" => title = value.to_string(),
            "BPM" => bpm = parse_decimal(value),
            "GAP" => gap_ms = parse_decimal(value).unwrap_or(0.0),
            "RELATIVE" if matches!(value.to_ascii_lowercase().as_str(), "yes" | "true") => {
                return None; // relative-beat mode unsupported
            }
            _ => {}
        }
    }

    let bpm = bpm.filter(|&b| b > 0.0)?;
    let beat_ms = 60_000.0 / (bpm * 4.0);
    let beat_to_secs = |beat: f64| (gap_ms + beat * beat_ms) / 1000.0;

    for raw in input.lines() {
        let line = raw.trim_end();
        let mut chars = line.chars();
        let Some(kind) = chars.next() else { continue };
        let body = chars.as_str();
        match kind {
            ':' | '*' | 'F' | 'R' | 'G' => {
                if let Some((start_beat, length, pitch, text)) = parse_note_body(body) {
                    notes.push(NoteEvent {
                        start: beat_to_secs(start_beat as f64),
                        end: beat_to_secs((start_beat + length.max(0)) as f64),
                        midi: (pitch + 60) as f64,
                        text,
                        golden: kind == '*',
                    });
                }
            }
            '-' => {
                // A line break carries the beat at which the next line starts.
                if let Some((beat, _rest)) = take_int(body.trim_start()) {
                    breaks.push(beat_to_secs(beat as f64));
                }
            }
            'E' => break,
            _ => {}
        }
    }

    if notes.is_empty() {
        return None;
    }
    Some(UltraStarSong {
        artist,
        title,
        gap_ms,
        bpm,
        notes,
        breaks,
    })
}

/// Parse a number with either `.` or `,` as the decimal separator.
fn parse_decimal(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse().ok()
}

/// Parse `<startBeat> <length> <pitch> <syllable…>` from a note line's body.
/// The syllable keeps its leading/internal spacing after the field separator.
fn parse_note_body(body: &str) -> Option<(i64, i64, i64, String)> {
    let (start, rest) = take_int(body.trim_start())?;
    let (length, rest) = take_int(rest)?;
    let (pitch, rest) = take_int(rest)?;
    // One separator space follows the pitch; anything beyond it is the syllable.
    let text = rest.strip_prefix(' ').unwrap_or(rest).to_string();
    Some((start, length, pitch, text))
}

/// Read a leading (optionally signed) integer after skipping spaces, returning
/// the value and the unconsumed remainder (its separator space included).
fn take_int(s: &str) -> Option<(i64, &str)> {
    let s = s.trim_start_matches(' ');
    let end = s
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || (c == '-' && i == 0)))
        .map_or(s.len(), |(i, _)| i);
    if end == 0 {
        return None;
    }
    let n: i64 = s[..end].parse().ok()?;
    Some((n, &s[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "#TITLE:Test Song\n\
#ARTIST:Test Artist\n\
#BPM:120\n\
#GAP:1000\n\
: 0 4 0 Hel\n\
: 4 4 2 lo\n\
* 8 8 7  world\n\
- 16\n\
: 16 4 -3 bye\n\
E\n";

    #[test]
    fn parses_headers_and_notes() {
        let song = parse_ultrastar(SAMPLE).unwrap();
        assert_eq!(song.artist, "Test Artist");
        assert_eq!(song.title, "Test Song");
        assert_eq!(song.bpm, 120.0);
        assert_eq!(song.gap_ms, 1000.0);
        assert_eq!(song.notes.len(), 4);
        assert_eq!(song.breaks.len(), 1);
    }

    #[test]
    fn computes_time_and_midi() {
        let song = parse_ultrastar(SAMPLE).unwrap();
        // beat_ms = 60000 / (120*4) = 125ms; gap 1000ms.
        // first note: start beat 0 -> 1.0s, length 4 -> end 1.5s, pitch 0 -> C4 (60).
        let n0 = &song.notes[0];
        assert!((n0.start - 1.0).abs() < 1e-9);
        assert!((n0.end - 1.5).abs() < 1e-9);
        assert_eq!(n0.midi, 60.0);
        assert_eq!(n0.text, "Hel");
        // golden note keeps its leading space inside the syllable.
        let n2 = &song.notes[2];
        assert!(n2.golden);
        assert_eq!(n2.midi, 67.0); // pitch 7 -> G4
        assert_eq!(n2.text, " world");
        // negative pitch -> below C4.
        assert_eq!(song.notes[3].midi, 57.0); // pitch -3 -> A3
    }

    #[test]
    fn accepts_comma_decimal_bpm() {
        let txt = "#TITLE:T\n#ARTIST:A\n#BPM:122,5\n: 0 1 0 x\nE\n";
        let song = parse_ultrastar(txt).unwrap();
        assert_eq!(song.bpm, 122.5);
    }

    #[test]
    fn rejects_relative_mode() {
        let txt = "#TITLE:T\n#ARTIST:A\n#BPM:120\n#RELATIVE:YES\n: 0 1 0 x\nE\n";
        assert!(parse_ultrastar(txt).is_none());
    }

    #[test]
    fn none_without_bpm_or_notes() {
        assert!(parse_ultrastar("#TITLE:T\n#ARTIST:A\n: 0 1 0 x\n").is_none());
        assert!(parse_ultrastar("#TITLE:T\n#ARTIST:A\n#BPM:120\nE\n").is_none());
        assert!(parse_ultrastar("").is_none());
    }

    #[test]
    fn stops_at_end_marker() {
        let txt = "#BPM:120\n#TITLE:T\n#ARTIST:A\n: 0 1 0 a\nE\n: 4 1 0 ignored\n";
        let song = parse_ultrastar(txt).unwrap();
        assert_eq!(song.notes.len(), 1);
    }
}
