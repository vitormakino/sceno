//! MPRIS (D-Bus) player tracking.
//!
//! Polls the active media player for now-playing metadata and position, fetches
//! synced lyrics from LRCLIB on track changes, and feeds the existing timeline
//! machinery via `Message::CuesReceived` / `Message::SyncReceived`.

use crate::{Message, TimelineSync, lrc, lrclib};
use futures::channel::mpsc::UnboundedSender;
use lrclib::TrackQuery;
use mpris::{PlaybackStatus, PlayerFinder};
use std::time::{Duration, Instant};

/// How often to re-anchor position from MPRIS while a track is playing.
const POLL_INTERVAL: Duration = Duration::from_millis(1000);
/// Backoff when no player is present.
const IDLE_INTERVAL: Duration = Duration::from_secs(2);
/// Times to re-attempt a lyrics fetch (one per poll) when a track first comes
/// back empty — covers transient LRCLIB outages that outlast the HTTP retries.
const FETCH_RETRIES: u32 = 3;

/// Noise words that mark a bracketed segment of a title as decoration.
const NOISE_WORDS: &[&str] = &[
    "official",
    "video",
    "audio",
    "lyric",
    "visualizer",
    "remaster",
    "explicit",
    "hd",
    "4k",
    "mv",
    "clip",
    "color",
    "colour",
    "performance",
    "version",
];

/// Build a lyrics query from raw MPRIS metadata fields, cleaning the decorations
/// that otherwise wreck LRCLIB lookups: bracketed tags (`[OFFICIAL VIDEO]`),
/// featuring suffixes (`ft. …`), and channel-name artists.
///
/// `from_browser` flags an unreliable artist field: browser tabs report the
/// channel/label (`Roadrunner Records`, `systemofadownVEVO`) rather than the
/// performer, but their title is reliably `Artist - Song` — so we take the
/// artist from the title and ignore the metadata. Native players (Spotify, mpv)
/// have trustworthy tags, so we keep their artist and leave the title intact
/// (safe for `Numb - Remastered`). Returns `None` if there's no usable title.
pub fn build_query(
    title: Option<String>,
    artists: Vec<String>,
    album: Option<String>,
    length_secs: Option<u64>,
    from_browser: bool,
) -> Option<TrackQuery> {
    let raw_title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())?;

    let meta_artist = artists
        .into_iter()
        .map(|a| clean_artist(&a))
        .find(|a| !a.is_empty());

    let (artist, title) = resolve_artist_title(clean_title(&raw_title), meta_artist, from_browser);

    Some(TrackQuery {
        artist,
        title,
        album: album
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty()),
        duration: length_secs.filter(|&d| d > 0),
    })
}

/// Resolve artist/title from a cleaned title and an optional metadata artist.
/// For browsers (and when no metadata artist is known) an `Artist - Song` title
/// is split; otherwise the metadata artist is trusted and the title left intact.
fn resolve_artist_title(
    title: String,
    meta_artist: Option<String>,
    from_browser: bool,
) -> (String, String) {
    if (from_browser || meta_artist.is_none())
        && let Some((left, right)) = title.split_once(" - ")
    {
        let (left, right) = (left.trim(), right.trim());
        if !left.is_empty() && !right.is_empty() {
            return (left.to_string(), right.to_string());
        }
    }
    (meta_artist.unwrap_or_default(), title)
}

/// Strip channel-name cruft from an artist: a `VEVO` suffix or a ` - Topic`
/// suffix (YouTube auto-generated artist channels).
fn clean_artist(s: &str) -> String {
    let mut a = s.trim().to_string();
    if a.to_lowercase().ends_with(" - topic") {
        a.truncate(a.len() - " - topic".len());
        a = a.trim().to_string();
    }
    if a.len() >= 4 && a[a.len() - 4..].eq_ignore_ascii_case("vevo") {
        a.truncate(a.len() - 4);
        a = a.trim_end().to_string();
    }
    a
}

/// Remove YouTube decorations from a title: all `[…]` tags, `(…)` groups that
/// contain a noise word, and any `feat./ft.` suffix.
fn clean_title(s: &str) -> String {
    let no_square = remove_groups(s, '[', ']', |_| false);
    let no_paren = remove_groups(&no_square, '(', ')', |inner| !contains_noise(inner));
    let no_feat = strip_featuring(&no_paren);
    no_feat
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| c == '-' || c.is_whitespace())
        .to_string()
}

fn contains_noise(inner: &str) -> bool {
    let lower = inner.to_lowercase();
    NOISE_WORDS.iter().any(|w| lower.contains(w))
}

/// Whether an MPRIS player identity is a web browser (whose artist metadata is
/// the channel/label, not the performer).
fn is_browser(identity: &str) -> bool {
    let id = identity.to_lowercase();
    [
        "chrome", "chromium", "firefox", "mozilla", "brave", "edge", "vivaldi", "opera",
    ]
    .iter()
    .any(|b| id.contains(b))
}

/// Remove `open…close` groups, keeping a group only when `keep_if(inner)` holds.
fn remove_groups(s: &str, open: char, close: char, keep_if: impl Fn(&str) -> bool) -> String {
    let mut out = String::new();
    let mut buf = String::new();
    let mut depth = 0u32;
    for c in s.chars() {
        if c == open {
            depth += 1;
        } else if c == close && depth > 0 {
            depth -= 1;
            if depth == 0 {
                if keep_if(&buf) {
                    out.push(open);
                    out.push_str(&buf);
                    out.push(close);
                }
                buf.clear();
            }
        } else if depth > 0 {
            buf.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

/// Truncate a title at the first `feat./ft./featuring` marker.
fn strip_featuring(s: &str) -> String {
    let markers = [" feat.", " feat ", " ft.", " ft ", " featuring "];
    match markers.iter().filter_map(|m| find_ci(s, m)).min() {
        Some(i) => s[..i].to_string(),
        None => s.to_string(),
    }
}

/// ASCII-case-insensitive substring search returning a byte index into `haystack`.
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// Lightweight stderr tracing, enabled with `SCENO_DEBUG=1`.
fn debug(args: std::fmt::Arguments) {
    if std::env::var_os("SCENO_DEBUG").is_some() {
        eprintln!("[player] {args}");
    }
}

/// Run the MPRIS polling loop forever (intended to own a dedicated thread).
pub fn run(tx: UnboundedSender<Message>) {
    loop {
        match PlayerFinder::new() {
            Ok(finder) => track_active_player(&finder, &tx),
            Err(e) => debug(format_args!("no D-Bus / PlayerFinder error: {e}")),
        }
        // Either no D-Bus or the player vanished — back off and retry.
        std::thread::sleep(IDLE_INTERVAL);
    }
}

/// Follow the active player until it disappears or errors, then return so the
/// caller can re-discover.
fn track_active_player(finder: &PlayerFinder, tx: &UnboundedSender<Message>) {
    let mut current_key: Option<String> = None;
    let mut retries_left: u32 = 0;

    loop {
        let player = match finder.find_active() {
            Ok(p) => p,
            Err(e) => {
                debug(format_args!("no active player: {e}"));
                return;
            }
        };
        let Ok(metadata) = player.get_metadata() else {
            debug(format_args!(
                "metadata read failed for '{}'",
                player.identity()
            ));
            return;
        };

        let query = build_query(
            metadata.title().map(str::to_string),
            metadata
                .artists()
                .map(|a| a.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default(),
            metadata.album_name().map(str::to_string),
            metadata.length().map(|d| d.as_secs()),
            is_browser(player.identity()),
        );

        let paused = !matches!(
            player
                .get_playback_status()
                .unwrap_or(PlaybackStatus::Stopped),
            PlaybackStatus::Playing
        );
        let position = player
            .get_position()
            .unwrap_or(Duration::ZERO)
            .as_secs_f64();
        let rate = player.get_playback_rate().unwrap_or(1.0);
        let sync = TimelineSync {
            video_time: position,
            captured_at: Instant::now(),
            paused,
            playback_rate: if rate > 0.0 { rate } else { 1.0 },
        };

        let key = query.as_ref().map(TrackQuery::key);
        let track_changed = key != current_key;
        if track_changed {
            current_key = key;
            retries_left = FETCH_RETRIES;
        }

        // Fetch on a new track, or while retrying one that came back empty.
        let send_result = if track_changed || retries_left > 0 {
            let cues = fetch_cues(&query);
            if !cues.is_empty() {
                retries_left = 0;
                debug(format_args!(
                    "player='{}' query={:?} -> {} cues",
                    player.identity(),
                    query,
                    cues.len()
                ));
                tx.unbounded_send(Message::CuesReceived(cues, sync))
            } else if track_changed {
                // First miss — keep the empty timeline but schedule retries.
                debug(format_args!("query={:?} -> no lyrics (will retry)", query));
                tx.unbounded_send(Message::CuesReceived(cues, sync))
            } else {
                retries_left -= 1;
                debug(format_args!("retry: still no lyrics ({retries_left} left)"));
                tx.unbounded_send(Message::SyncReceived(sync))
            }
        } else {
            tx.unbounded_send(Message::SyncReceived(sync))
        };

        if send_result.is_err() {
            return; // app shutting down
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Fetch and parse synced lyrics for a query, or an empty list on miss/failure.
fn fetch_cues(query: &Option<TrackQuery>) -> Vec<crate::CueEntry> {
    query
        .as_ref()
        .and_then(lrclib::fetch_synced)
        .map(|s| lrc::parse_lrc(&s))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_query_uses_explicit_artist() {
        // Native player (trustworthy tags): keep the metadata artist.
        let q = build_query(
            Some("Dreams".into()),
            vec!["Fleetwood Mac".into()],
            Some("Rumours".into()),
            Some(257),
            false,
        )
        .unwrap();
        assert_eq!(q.artist, "Fleetwood Mac");
        assert_eq!(q.title, "Dreams");
        assert_eq!(q.album.as_deref(), Some("Rumours"));
        assert_eq!(q.duration, Some(257));
    }

    #[test]
    fn build_query_splits_artist_from_title_when_missing() {
        let q = build_query(
            Some("Daft Punk - Get Lucky".into()),
            vec![],
            None,
            None,
            false,
        )
        .unwrap();
        assert_eq!(q.artist, "Daft Punk");
        assert_eq!(q.title, "Get Lucky");
    }

    #[test]
    fn build_query_keeps_title_when_no_separator() {
        let q = build_query(Some("Untitled".into()), vec![], None, None, true).unwrap();
        assert_eq!(q.artist, "");
        assert_eq!(q.title, "Untitled");
    }

    #[test]
    fn build_query_ignores_blank_artists() {
        let q = build_query(Some("Song".into()), vec!["   ".into()], None, None, false).unwrap();
        // blank artist falls through to title-splitting, which has no separator
        assert_eq!(q.artist, "");
        assert_eq!(q.title, "Song");
    }

    #[test]
    fn build_query_none_without_title() {
        assert!(build_query(None, vec!["Artist".into()], None, None, false).is_none());
        assert!(build_query(Some("  ".into()), vec![], None, None, true).is_none());
    }

    #[test]
    fn build_query_drops_zero_duration_and_empty_album() {
        let q = build_query(
            Some("S".into()),
            vec!["A".into()],
            Some("".into()),
            Some(0),
            false,
        )
        .unwrap();
        assert_eq!(q.album, None);
        assert_eq!(q.duration, None);
    }

    // ── metadata cleaning ─────────────────────────────────────────────────────

    #[test]
    fn clean_artist_strips_vevo_and_topic() {
        assert_eq!(clean_artist("TimbalandVEVO"), "Timbaland");
        assert_eq!(clean_artist("EminemVEVO"), "Eminem");
        assert_eq!(clean_artist("OneRepublic - Topic"), "OneRepublic");
        assert_eq!(clean_artist("Slipknot"), "Slipknot");
    }

    #[test]
    fn clean_title_removes_brackets_and_featuring() {
        assert_eq!(
            clean_title("Slipknot - Vermilion Pt. 2 [OFFICIAL VIDEO] [HD]"),
            "Slipknot - Vermilion Pt. 2"
        );
        assert_eq!(clean_title("Apologize ft. OneRepublic"), "Apologize");
        assert_eq!(clean_title("Song (Official Audio)"), "Song");
    }

    #[test]
    fn clean_title_keeps_meaningful_parentheses() {
        // a parenthetical without noise words is part of the real title
        assert_eq!(clean_title("Hurt (Acoustic)"), "Hurt (Acoustic)");
    }

    #[test]
    fn build_query_browser_takes_artist_from_title() {
        // Browser tab: the channel/VEVO artist is ignored; the title is split.
        // (Featuring suffix and decorations are cleaned first.)
        let q = build_query(
            Some("Timbaland - Apologize ft. OneRepublic".into()),
            vec!["TimbalandVEVO".into()],
            None,
            Some(188),
            true,
        )
        .unwrap();
        assert_eq!(q.artist, "Timbaland");
        assert_eq!(q.title, "Apologize");
    }

    #[test]
    fn build_query_browser_strips_decorations_and_prefix() {
        let q = build_query(
            Some("Slipknot - Vermilion Pt. 2 [OFFICIAL VIDEO] [HD]".into()),
            vec!["Slipknot".into()],
            None,
            Some(232),
            true,
        )
        .unwrap();
        assert_eq!(q.artist, "Slipknot");
        assert_eq!(q.title, "Vermilion Pt. 2");
    }

    #[test]
    fn build_query_browser_ignores_label_artist() {
        // Observed live: Chrome reported the record label as the artist, while
        // the real performer was the title prefix. The title must win.
        let q = build_query(
            Some("Stone Sour - Through Glass".into()),
            vec!["Roadrunner Records".into()],
            None,
            Some(257),
            true,
        )
        .unwrap();
        assert_eq!(q.artist, "Stone Sour");
        assert_eq!(q.title, "Through Glass");
    }

    #[test]
    fn build_query_native_keeps_dash_title_intact() {
        // Spotify-style "Song - Remastered" from a native player: the metadata
        // artist is trusted and the title is not split.
        let q = build_query(
            Some("Numb - Remastered".into()),
            vec!["Linkin Park".into()],
            None,
            Some(187),
            false,
        )
        .unwrap();
        assert_eq!(q.artist, "Linkin Park");
        assert_eq!(q.title, "Numb - Remastered");
    }

    #[test]
    fn is_browser_detects_common_browsers() {
        for id in ["Chrome", "Chromium", "Mozilla Firefox", "Brave"] {
            assert!(is_browser(id), "{id} should be a browser");
        }
        for id in ["Spotify", "mpv", "VLC media player"] {
            assert!(!is_browser(id), "{id} should not be a browser");
        }
    }

    #[test]
    fn fetch_cues_empty_for_missing_query() {
        // A `None` query short-circuits before any network access.
        assert!(fetch_cues(&None).is_empty());
    }
}
