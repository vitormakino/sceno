//! A local folder of lyrics files (`.txt` UltraStar, `.lrc` LRCLIB), matched to
//! the now-playing track by a normalized artist/title key.

use std::path::{Path, PathBuf};

use crate::cue::CueEntry;
use crate::lrc;
use crate::lrclib::TrackQuery;
use crate::ultrastar::{self, UltraStarSong};

/// Which kind of lyrics file an entry points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// UltraStar `.txt` — carries per-syllable pitch (drives the karaoke view).
    UltraStar,
    /// LRCLIB `.lrc` — caption-only timing.
    Lrc,
}

/// An indexed lyrics file in the library.
#[derive(Debug, Clone)]
pub struct LibraryEntry {
    pub artist: String,
    pub title: String,
    pub path: PathBuf,
    pub kind: Kind,
    /// Normalized `artist|title` match key.
    key: String,
}

/// A loaded song: either pitched UltraStar notes or caption-only cues.
pub enum Song {
    UltraStar(UltraStarSong),
    Lrc { cues: Vec<CueEntry> },
}

impl Song {
    /// The UltraStar song if this is one (the karaoke view needs notes).
    pub fn into_ultrastar(self) -> Option<UltraStarSong> {
        match self {
            Song::UltraStar(s) => Some(s),
            Song::Lrc { .. } => None,
        }
    }
}

/// Scan `dir` for `.txt`/`.lrc` files, returning an index entry per readable one.
/// Unreadable or unidentifiable files are skipped; a missing dir yields empty.
pub fn scan(dir: &Path) -> Vec<LibraryEntry> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        let identified = match ext.as_deref() {
            Some("txt") => ultrastar_headers(&path).map(|(a, t)| (a, t, Kind::UltraStar)),
            Some("lrc") => lrc_identity(&path).map(|(a, t)| (a, t, Kind::Lrc)),
            _ => None,
        };
        if let Some((artist, title, kind)) = identified {
            let key = norm_key(&artist, &title);
            out.push(LibraryEntry {
                artist,
                title,
                path,
                kind,
                key,
            });
        }
    }
    out
}

/// Find the library entry best matching `q`: exact `artist|title` first, then a
/// title-only fallback (browser artists are noisy). UltraStar is preferred over
/// LRC when both match, since only it can drive the karaoke pitch view.
pub fn match_track<'a>(entries: &'a [LibraryEntry], q: &TrackQuery) -> Option<&'a LibraryEntry> {
    let key = norm_key(&q.artist, &q.title);
    let by_key = || entries.iter().filter(|e| e.key == key);
    if let Some(e) = prefer_ultrastar(by_key()) {
        return Some(e);
    }
    let title = normalize(&q.title);
    let by_title = || entries.iter().filter(|e| normalize(&e.title) == title);
    prefer_ultrastar(by_title())
}

/// Read a library entry off disk into a playable [`Song`].
pub fn load(entry: &LibraryEntry) -> Option<Song> {
    let text = std::fs::read_to_string(&entry.path).ok()?;
    match entry.kind {
        Kind::UltraStar => ultrastar::parse_ultrastar(&text).map(Song::UltraStar),
        Kind::Lrc => {
            let cues = lrc::parse_lrc(&text);
            (!cues.is_empty()).then_some(Song::Lrc { cues })
        }
    }
}

/// From matching candidates, take the first UltraStar one, else the first at all.
fn prefer_ultrastar<'a>(
    mut it: impl Iterator<Item = &'a LibraryEntry> + Clone,
) -> Option<&'a LibraryEntry> {
    it.clone()
        .find(|e| e.kind == Kind::UltraStar)
        .or_else(|| it.next())
}

/// Normalized `artist|title` match key.
fn norm_key(artist: &str, title: &str) -> String {
    format!("{}|{}", normalize(artist), normalize(title))
}

/// Lowercase and strip to `[a-z0-9]` so punctuation/spacing/case don't block a
/// match (e.g. `"Beyoncé!"` ~ `"beyonce"`).
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Cheaply read `#ARTIST`/`#TITLE` from an UltraStar `.txt` without full parse.
/// Returns `None` if no title is found.
fn ultrastar_headers(path: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut artist = String::new();
    let mut title = String::new();
    for line in text.lines() {
        let line = line.trim_end();
        let Some(rest) = line.strip_prefix('#') else {
            break; // headers precede notes; stop at the first non-header line
        };
        if let Some((key, value)) = rest.split_once(':') {
            match key.trim().to_ascii_uppercase().as_str() {
                "ARTIST" => artist = value.trim().to_string(),
                "TITLE" => title = value.trim().to_string(),
                _ => {}
            }
        }
    }
    (!title.is_empty()).then_some((artist, title))
}

/// Identify an `.lrc` by its `Artist - Title.lrc` filename, falling back to its
/// `[ar:]`/`[ti:]` metadata tags.
fn lrc_identity(path: &Path) -> Option<(String, String)> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if let Some((artist, title)) = stem.split_once(" - ") {
        let (artist, title) = (artist.trim(), title.trim());
        if !title.is_empty() {
            return Some((artist.to_string(), title.to_string()));
        }
    }
    let text = std::fs::read_to_string(path).ok()?;
    let (mut artist, mut title) = (String::new(), String::new());
    for line in text.lines() {
        if let Some(v) = lrc_tag(line, "ar") {
            artist = v;
        } else if let Some(v) = lrc_tag(line, "ti") {
            title = v;
        }
    }
    if title.is_empty() && !stem.is_empty() {
        title = stem.to_string(); // last resort: the bare filename
    }
    (!title.is_empty()).then_some((artist, title))
}

/// Extract the value of an LRC metadata tag like `[ar:Artist]`.
fn lrc_tag(line: &str, tag: &str) -> Option<String> {
    let inner = line.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (key, value) = inner.split_once(':')?;
    (key.trim().eq_ignore_ascii_case(tag)).then(|| value.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("sceno-media-test-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn query(artist: &str, title: &str) -> TrackQuery {
        TrackQuery {
            artist: artist.into(),
            title: title.into(),
            album: None,
            duration: None,
        }
    }

    #[test]
    fn normalize_strips_case_and_punctuation() {
        assert_eq!(normalize("Beyoncé!"), "beyonc");
        assert_eq!(normalize("AC/DC"), "acdc");
        assert_eq!(norm_key("Daft Punk", "Get Lucky"), "daftpunk|getlucky");
    }

    #[test]
    fn scans_matches_and_prefers_ultrastar() {
        let dir = temp_dir();
        std::fs::write(
            dir.join("Test Artist - Test Song.lrc"),
            "[ar:Test Artist]\n[ti:Test Song]\n[00:01.00]hi\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("song.txt"),
            "#ARTIST:Test Artist\n#TITLE:Test Song\n#BPM:120\n: 0 4 0 hi\nE\n",
        )
        .unwrap();

        let entries = scan(&dir);
        assert_eq!(entries.len(), 2);

        // Exact match prefers the UltraStar entry.
        let m = match_track(&entries, &query("test artist", "test song")).unwrap();
        assert_eq!(m.kind, Kind::UltraStar);
        assert!(matches!(load(m), Some(Song::UltraStar(_))));

        // Punctuation/case differences still match.
        assert!(match_track(&entries, &query("Test  Artist!", "Test-Song")).is_some());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn matches_title_only_when_artist_differs() {
        let dir = temp_dir();
        std::fs::write(dir.join("Real Artist - Lonely Title.lrc"), "[00:01.00]x\n").unwrap();
        let entries = scan(&dir);
        // Browser reported a label as artist, but the title still matches.
        let m = match_track(&entries, &query("Some Label", "Lonely Title")).unwrap();
        assert_eq!(m.title, "Lonely Title");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_dir_is_empty() {
        assert!(scan(Path::new("/nonexistent/sceno/songs/xyz")).is_empty());
    }
}
