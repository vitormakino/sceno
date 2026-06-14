//! Fetching synced lyrics from [LRCLIB](https://lrclib.net/docs).
//!
//! Strategy: try `/api/get` (exact signature incl. duration), then fall back to
//! `/api/search`, picking the candidate whose duration is closest. Results are
//! cached on disk to avoid re-fetching and to be kind to the free API.

use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

const BASE: &str = "https://lrclib.net";
const USER_AGENT: &str = concat!(
    "sceno-lyrics v",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/vitormakino/sceno)"
);

/// Extra attempts after the first on a transient failure (network error / 5xx).
const HTTP_RETRIES: u32 = 2;
const RETRY_BACKOFF: Duration = Duration::from_millis(400);

/// Identifying signature for a track, derived from player metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackQuery {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub duration: Option<u64>,
}

impl TrackQuery {
    /// Stable key for change-detection and cache filenames.
    pub fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.artist.to_lowercase(),
            self.title.to_lowercase(),
            self.album.as_deref().unwrap_or("").to_lowercase(),
            self.duration.unwrap_or(0)
        )
    }
}

/// A single LRCLIB track record.
#[derive(Deserialize, Debug, Clone, Default)]
pub struct LrclibTrack {
    #[serde(default)]
    pub instrumental: bool,
    #[serde(rename = "syncedLyrics", default)]
    pub synced_lyrics: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
}

impl LrclibTrack {
    fn usable_synced(&self) -> Option<String> {
        self.synced_lyrics.clone().filter(|s| !s.trim().is_empty())
    }
}

/// Fetch the synced LRC string for a track, or `None` if unavailable.
/// Consults the on-disk cache first, then the network.
pub fn fetch_synced(q: &TrackQuery) -> Option<String> {
    if let Some(cached) = cache_read(q) {
        return Some(cached);
    }
    // Cache only successful lookups: a miss may be a transient network error or
    // lyrics simply not published yet, so later plays should retry.
    let lrc = network_fetch(q);
    if let Some(s) = &lrc {
        cache_write(q, s);
        persist_to_library(q, s);
    }
    lrc
}

fn network_fetch(q: &TrackQuery) -> Option<String> {
    if let Some(track) = api_get(q) {
        if let Some(s) = track.usable_synced() {
            return Some(s);
        }
        if track.instrumental {
            return None; // exact match is instrumental — no point searching
        }
    }
    best_match(api_search(q), q.duration).and_then(|t| t.usable_synced())
}

fn api_get(q: &TrackQuery) -> Option<LrclibTrack> {
    let body = call_with_retry(|| {
        let mut req = ureq::get(&format!("{BASE}/api/get"))
            .set("User-Agent", USER_AGENT)
            .query("artist_name", &q.artist)
            .query("track_name", &q.title);
        if let Some(album) = &q.album {
            req = req.query("album_name", album);
        }
        if let Some(d) = q.duration {
            req = req.query("duration", &d.to_string());
        }
        req
    })?;
    serde_json::from_str(&body).ok()
}

fn api_search(q: &TrackQuery) -> Vec<LrclibTrack> {
    call_with_retry(|| {
        ureq::get(&format!("{BASE}/api/search"))
            .set("User-Agent", USER_AGENT)
            .query("track_name", &q.title)
            .query("artist_name", &q.artist)
    })
    .and_then(|b| serde_json::from_str(&b).ok())
    .unwrap_or_default()
}

/// Whether an HTTP status code warrants a retry. Only server-side `5xx`
/// errors are transient; `4xx` (including `404`/`429`) are definitive.
fn status_is_retryable(code: u16) -> bool {
    (500..600).contains(&code)
}

/// Issue a request, retrying only transient failures (transport/network errors
/// and `5xx`) with a short backoff. Any `4xx` is a definitive result and
/// returns immediately. The request is rebuilt per attempt since
/// `ureq::Request` is single-use.
fn call_with_retry(build: impl Fn() -> ureq::Request) -> Option<String> {
    for attempt in 0..=HTTP_RETRIES {
        match build().call() {
            Ok(resp) => return resp.into_string().ok(),
            Err(ureq::Error::Status(code, _))
                if status_is_retryable(code) && attempt < HTTP_RETRIES =>
            {
                std::thread::sleep(RETRY_BACKOFF);
            }
            Err(ureq::Error::Transport(_)) if attempt < HTTP_RETRIES => {
                std::thread::sleep(RETRY_BACKOFF);
            }
            // 4xx, a non-retryable status, or retries exhausted.
            Err(_) => return None,
        }
    }
    None
}

/// Among candidates with usable synced lyrics, pick the one whose duration is
/// closest to `target` (or the first, if no target duration is known).
fn best_match(tracks: Vec<LrclibTrack>, target: Option<u64>) -> Option<LrclibTrack> {
    let mut usable = tracks.into_iter().filter(|t| t.usable_synced().is_some());
    match target {
        Some(d) => {
            usable.min_by_key(|t| (t.duration.unwrap_or(0.0) - d as f64).abs().round() as i64)
        }
        None => usable.next(),
    }
}

// ── On-disk cache ───────────────────────────────────────────────────────────

fn cache_dir() -> Option<PathBuf> {
    overlay::cache_dir("lyrics")
}

fn cache_file(q: &TrackQuery) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    q.key().hash(&mut hasher);
    cache_dir().map(|d| d.join(format!("{:016x}.lrc", hasher.finish())))
}

fn cache_read(q: &TrackQuery) -> Option<String> {
    if cfg!(test) {
        return None;
    }
    std::fs::read_to_string(cache_file(q)?)
        .ok()
        .filter(|s| !s.is_empty())
}

fn cache_write(q: &TrackQuery, contents: &str) {
    if cfg!(test) {
        return;
    }
    let Some(path) = cache_file(q) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, contents);
}

// ── Local song library ──────────────────────────────────────────────────────

/// `Artist - Title.lrc` (sanitized) — the filename convention the library reads.
fn library_filename(q: &TrackQuery) -> String {
    let sanitize = |s: &str| {
        s.chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':') {
                    '-'
                } else {
                    c
                }
            })
            .collect::<String>()
            .trim()
            .to_string()
    };
    let artist = sanitize(&q.artist);
    let title = sanitize(&q.title);
    if artist.is_empty() {
        format!("{title}.lrc")
    } else {
        format!("{artist} - {title}.lrc")
    }
}

/// Save a freshly-fetched LRC into the shared song library so it is reused
/// without re-downloading (and is matchable by the UltraStar/lyrics library).
/// Never overwrites an existing file (e.g. a curated UltraStar `.txt` companion).
fn persist_to_library(q: &TrackQuery, contents: &str) {
    if cfg!(test) {
        return;
    }
    let Some(dir) = overlay::songs_dir() else {
        return;
    };
    let path = dir.join(library_filename(q));
    if path.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(path, contents);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q() -> TrackQuery {
        TrackQuery {
            artist: "Fleetwood Mac".into(),
            title: "Dreams".into(),
            album: Some("Rumours".into()),
            duration: Some(257),
        }
    }

    #[test]
    fn key_is_case_insensitive_and_stable() {
        let a = TrackQuery {
            artist: "ABBA".into(),
            title: "SOS".into(),
            album: None,
            duration: Some(200),
        };
        let b = TrackQuery {
            artist: "abba".into(),
            title: "sos".into(),
            album: None,
            duration: Some(200),
        };
        assert_eq!(a.key(), b.key());
        assert_eq!(a.key(), "abba|sos||200");
    }

    #[test]
    fn deserializes_api_get_response() {
        let json = r#"{
            "id": 3396226,
            "trackName": "Dreams",
            "artistName": "Fleetwood Mac",
            "albumName": "Rumours",
            "duration": 257.0,
            "instrumental": false,
            "plainLyrics": "Now here you go again",
            "syncedLyrics": "[00:15.00]Now here you go again"
        }"#;
        let t: LrclibTrack = serde_json::from_str(json).unwrap();
        assert_eq!(
            t.usable_synced().as_deref(),
            Some("[00:15.00]Now here you go again")
        );
        assert!(!t.instrumental);
    }

    #[test]
    fn instrumental_track_has_no_usable_synced() {
        let json = r#"{ "instrumental": true, "syncedLyrics": null, "plainLyrics": null }"#;
        let t: LrclibTrack = serde_json::from_str(json).unwrap();
        assert!(t.usable_synced().is_none());
    }

    #[test]
    fn empty_synced_is_not_usable() {
        let json = r#"{ "syncedLyrics": "   " }"#;
        let t: LrclibTrack = serde_json::from_str(json).unwrap();
        assert!(t.usable_synced().is_none());
    }

    #[test]
    fn best_match_picks_closest_duration() {
        let tracks = vec![
            LrclibTrack {
                duration: Some(300.0),
                synced_lyrics: Some("[00:01.00]a".into()),
                ..Default::default()
            },
            LrclibTrack {
                duration: Some(258.0),
                synced_lyrics: Some("[00:01.00]b".into()),
                ..Default::default()
            },
            LrclibTrack {
                duration: Some(200.0),
                synced_lyrics: Some("[00:01.00]c".into()),
                ..Default::default()
            },
        ];
        let picked = best_match(tracks, Some(257)).unwrap();
        assert_eq!(picked.synced_lyrics.as_deref(), Some("[00:01.00]b"));
    }

    #[test]
    fn best_match_skips_candidates_without_synced() {
        let tracks = vec![
            LrclibTrack {
                duration: Some(257.0),
                synced_lyrics: None,
                ..Default::default()
            },
            LrclibTrack {
                duration: Some(400.0),
                synced_lyrics: Some("[00:01.00]only".into()),
                ..Default::default()
            },
        ];
        let picked = best_match(tracks, Some(257)).unwrap();
        assert_eq!(picked.synced_lyrics.as_deref(), Some("[00:01.00]only"));
    }

    #[test]
    fn best_match_none_when_no_synced() {
        let tracks = vec![LrclibTrack {
            duration: Some(257.0),
            synced_lyrics: None,
            ..Default::default()
        }];
        assert!(best_match(tracks, Some(257)).is_none());
    }

    #[test]
    fn library_filename_is_artist_dash_title() {
        assert_eq!(library_filename(&q()), "Fleetwood Mac - Dreams.lrc");
        let no_artist = TrackQuery {
            artist: "".into(),
            title: "Solo".into(),
            album: None,
            duration: None,
        };
        assert_eq!(library_filename(&no_artist), "Solo.lrc");
        let slashed = TrackQuery {
            artist: "AC/DC".into(),
            title: "T.N.T".into(),
            album: None,
            duration: None,
        };
        assert_eq!(library_filename(&slashed), "AC-DC - T.N.T.lrc");
    }

    #[test]
    fn cache_disabled_in_tests() {
        // Guards against tests accidentally depending on a real cache dir.
        assert!(cache_read(&q()).is_none());
    }

    #[test]
    fn only_5xx_status_is_retryable() {
        // Client errors (4xx) are definitive — never retried.
        assert!(!status_is_retryable(400));
        assert!(!status_is_retryable(403));
        assert!(!status_is_retryable(404));
        assert!(!status_is_retryable(429));
        // Server errors (5xx) are transient — retry.
        assert!(status_is_retryable(500));
        assert!(status_is_retryable(503));
    }
}
