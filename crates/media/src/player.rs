//! Now-playing player tracking.
//!
//! Polls the active media player for now-playing metadata and position, fetches
//! synced lyrics from LRCLIB on track changes, and delivers [`PlayerEvent`]s to a
//! caller-supplied sink. Apps map these into their own message type.
//!
//! The *discovery* of the active player is platform-specific — **MPRIS** (D-Bus)
//! on Linux, an **AppleScript** backend (Music.app / Spotify.app via `osascript`)
//! elsewhere — but both feed the same shared [`Tracker`], so the track-change /
//! fetch-retry / event-emission logic lives in one place.

use crate::cue::CueEntry;
use crate::lrclib::TrackQuery;
use crate::sync::TimelineSync;
use crate::{lrc, lrclib};
#[cfg(target_os = "linux")]
use mpris::{PlaybackStatus, PlayerFinder};
use std::time::{Duration, Instant};

/// How often to re-anchor position from MPRIS while a track is playing.
const POLL_INTERVAL: Duration = Duration::from_millis(1000);
/// Backoff when no player is present.
const IDLE_INTERVAL: Duration = Duration::from_secs(2);
/// Times to re-attempt a lyrics fetch (one per poll) when a track first comes
/// back empty — covers transient LRCLIB outages that outlast the HTTP retries.
const FETCH_RETRIES: u32 = 3;

/// Now-playing update delivered to the app. Carries the resolved `query` (so an
/// app can do its own library lookup) alongside the LRCLIB `cues`.
pub enum PlayerEvent {
    /// A (possibly new) track: its query, any synced cues, and a position sample.
    Track {
        query: Option<TrackQuery>,
        cues: Vec<CueEntry>,
        sync: TimelineSync,
    },
    /// A position-only update for the current track.
    Sync(TimelineSync),
}

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
/// the channel/label, not the performer). Linux-only: the AppleScript backend
/// reads native players (Music/Spotify) whose tags are always trustworthy.
#[cfg(target_os = "linux")]
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
/// Thin wrapper over [`overlay::debug`] that fixes the `player` tag.
fn debug(args: std::fmt::Arguments) {
    overlay::debug("player", args);
}

/// A platform-neutral now-playing reading: the resolved query plus a position
/// sample. Both backends produce these; [`Tracker::step`] turns them into events.
struct Snapshot {
    query: Option<TrackQuery>,
    sync: TimelineSync,
}

/// Shared track-change + fetch-retry state, driven by snapshots from whichever
/// backend is active. Keeps the LRCLIB fetch logic in one place.
struct Tracker {
    current_key: Option<String>,
    retries_left: u32,
}

impl Tracker {
    fn new() -> Self {
        Tracker {
            current_key: None,
            retries_left: 0,
        }
    }

    /// Turn one snapshot into a [`PlayerEvent`] and hand it to `sink`. Fetches
    /// lyrics on a new track, and re-attempts (one per call) when a track first
    /// comes back empty. Returns `false` when the sink asks to stop.
    fn step(&mut self, snap: Snapshot, sink: &mut impl FnMut(PlayerEvent) -> bool) -> bool {
        let Snapshot { query, sync } = snap;
        let key = query.as_ref().map(TrackQuery::key);
        let track_changed = key != self.current_key;
        if track_changed {
            self.current_key = key;
            self.retries_left = FETCH_RETRIES;
        }

        // Fetch on a new track, or while retrying one that came back empty.
        let event = if track_changed || self.retries_left > 0 {
            let cues = fetch_cues(&query);
            if !cues.is_empty() {
                self.retries_left = 0;
                debug(format_args!("query={:?} -> {} cues", query, cues.len()));
                PlayerEvent::Track { query, cues, sync }
            } else if track_changed {
                // First miss — keep the empty timeline but schedule retries.
                debug(format_args!("query={:?} -> no lyrics (will retry)", query));
                PlayerEvent::Track { query, cues, sync }
            } else {
                self.retries_left -= 1;
                debug(format_args!(
                    "retry: still no lyrics ({} left)",
                    self.retries_left
                ));
                PlayerEvent::Sync(sync)
            }
        } else {
            PlayerEvent::Sync(sync)
        };

        sink(event)
    }
}

/// Fetch and parse synced lyrics for a query, or an empty list on miss/failure.
fn fetch_cues(query: &Option<TrackQuery>) -> Vec<CueEntry> {
    query
        .as_ref()
        .and_then(lrclib::fetch_synced)
        .map(|s| lrc::parse_lrc(&s))
        .unwrap_or_default()
}

// ── Linux backend: MPRIS (D-Bus) ────────────────────────────────────────────

/// Run the MPRIS polling loop forever (intended to own a dedicated thread).
/// `sink` returns `false` to stop (its receiver was dropped on app exit).
#[cfg(target_os = "linux")]
pub fn run(mut sink: impl FnMut(PlayerEvent) -> bool) {
    loop {
        match PlayerFinder::new() {
            Ok(finder) => {
                if !track_active_player(&finder, &mut sink) {
                    return;
                }
            }
            Err(e) => debug(format_args!("no D-Bus / PlayerFinder error: {e}")),
        }
        // Either no D-Bus or the player vanished — back off and retry.
        std::thread::sleep(IDLE_INTERVAL);
    }
}

/// Follow the active player until it disappears or errors, then return `true` so
/// the caller can re-discover. Returns `false` if the sink asked to stop.
#[cfg(target_os = "linux")]
fn track_active_player(finder: &PlayerFinder, sink: &mut impl FnMut(PlayerEvent) -> bool) -> bool {
    let mut tracker = Tracker::new();

    loop {
        let player = match finder.find_active() {
            Ok(p) => p,
            Err(e) => {
                debug(format_args!("no active player: {e}"));
                return true;
            }
        };
        let Ok(metadata) = player.get_metadata() else {
            debug(format_args!(
                "metadata read failed for '{}'",
                player.identity()
            ));
            return true;
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

        if !tracker.step(Snapshot { query, sync }, sink) {
            return false; // app shutting down
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}

// ── macOS (and other non-Linux) backend: AppleScript ────────────────────────

/// Run the AppleScript polling loop forever (intended to own a dedicated thread).
/// Reads Music.app, then Spotify.app, whichever is running and not stopped.
/// `sink` returns `false` to stop (its receiver was dropped on app exit).
#[cfg(not(target_os = "linux"))]
pub fn run(mut sink: impl FnMut(PlayerEvent) -> bool) {
    let mut tracker = Tracker::new();
    loop {
        match poll_now_playing() {
            Some(snap) => {
                if !tracker.step(snap, &mut sink) {
                    return; // app shutting down
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            None => {
                // Nothing playing (or no scriptable player) — back off and retry.
                std::thread::sleep(IDLE_INTERVAL);
            }
        }
    }
}

/// Player apps queried in priority order: native Music first, then Spotify.
#[cfg(not(target_os = "linux"))]
const SCRIPTABLE_PLAYERS: &[&str] = &["Music", "Spotify"];

/// Ask each scriptable player for its current track; return the first hit.
#[cfg(not(target_os = "linux"))]
fn poll_now_playing() -> Option<Snapshot> {
    SCRIPTABLE_PLAYERS
        .iter()
        .find_map(|app| parse_now_playing(&run_osascript(app), app))
}

/// Run the now-playing AppleScript for `app`, returning its trimmed stdout
/// (empty on any error, or when the app isn't running / is stopped).
#[cfg(not(target_os = "linux"))]
fn run_osascript(app: &str) -> String {
    match std::process::Command::new("osascript")
        .arg("-e")
        .arg(now_playing_script(app))
        .output()
    {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(e) => {
            // First call also surfaces a TCC Automation denial (osascript exits
            // non-zero with empty stdout); treat any failure as "nothing playing".
            debug(format_args!("osascript('{app}') failed: {e}"));
            String::new()
        }
    }
}

/// Build the guarded AppleScript that returns a pipe-delimited now-playing line,
/// or an empty string. The `is running` guard avoids auto-launching the app.
#[cfg(not(target_os = "linux"))]
fn now_playing_script(app: &str) -> String {
    format!(
        "if application \"{app}\" is running then\n\
         tell application \"{app}\"\n\
         if player state is not stopped then\n\
         set t to current track\n\
         return (name of t) & \"|\" & (artist of t) & \"|\" & (album of t) & \"|\" & (duration of t) & \"|\" & (player position) & \"|\" & (player state as text)\n\
         end if\n\
         end tell\n\
         end if\n\
         return \"\""
    )
}

/// Parse one pipe-delimited now-playing line (`name|artist|album|duration|position|state`)
/// into a [`Snapshot`]. Returns `None` for empty/short output. Spotify reports
/// `duration` in milliseconds (Music in seconds), so the unit is normalized by app.
#[cfg(not(target_os = "linux"))]
fn parse_now_playing(stdout: &str, app: &str) -> Option<Snapshot> {
    let line = stdout.trim();
    if line.is_empty() {
        return None;
    }
    let f: Vec<&str> = line.split('|').collect();
    if f.len() < 6 {
        debug(format_args!("unparsable now-playing line: {line:?}"));
        return None;
    }
    let title = f[0].trim().to_string();
    let artist = f[1].trim().to_string();
    let album = f[2].trim().to_string();
    // AppleScript formats reals per the system locale, so a pt-BR/EU Mac yields a
    // decimal comma ("199,80"). Normalize to a dot before parsing.
    let duration_raw: f64 = f[3].trim().replace(',', ".").parse().ok()?;
    let position: f64 = f[4].trim().replace(',', ".").parse().unwrap_or(0.0);
    let state = f[5].trim();

    // Spotify's `duration` is in ms; Music's is in seconds.
    let length_secs = if app.eq_ignore_ascii_case("Spotify") {
        (duration_raw / 1000.0) as u64
    } else {
        duration_raw as u64
    };

    let query = build_query(
        Some(title),
        if artist.is_empty() {
            Vec::new()
        } else {
            vec![artist]
        },
        Some(album),
        Some(length_secs),
        false, // native player: trustworthy tags, keep title intact
    );

    let paused = !state.eq_ignore_ascii_case("playing");
    let sync = TimelineSync {
        video_time: position,
        captured_at: Instant::now(),
        paused,
        playback_rate: 1.0,
    };
    Some(Snapshot { query, sync })
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

    #[cfg(target_os = "linux")]
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

// ── macOS AppleScript parsing ───────────────────────────────────────────────
// The subprocess can't run in CI, but the line parser is pure and testable; the
// macOS CI job (`check-macos`) compiles + runs these.
#[cfg(all(test, not(target_os = "linux")))]
mod macos_tests {
    use super::*;

    #[test]
    fn parses_music_line_seconds() {
        let snap = parse_now_playing(
            "Get Lucky|Daft Punk|Random Access Memories|248|12.5|playing",
            "Music",
        )
        .expect("should parse");
        let q = snap.query.expect("query");
        assert_eq!(q.artist, "Daft Punk");
        assert_eq!(q.title, "Get Lucky");
        assert_eq!(q.album.as_deref(), Some("Random Access Memories"));
        assert_eq!(q.duration, Some(248)); // Music: seconds, used as-is
        assert!(!snap.sync.paused);
        assert_eq!(snap.sync.video_time, 12.5);
    }

    #[test]
    fn spotify_duration_is_milliseconds() {
        // Spotify reports duration in ms; 248000 ms → 248 s.
        let snap = parse_now_playing("Get Lucky|Daft Punk|RAM|248000|0.0|playing", "Spotify")
            .expect("should parse");
        assert_eq!(snap.query.unwrap().duration, Some(248));
    }

    #[test]
    fn parses_comma_decimal_locale() {
        // A pt-BR/EU Mac formats AppleScript reals with a decimal comma.
        let snap = parse_now_playing(
            "Last Resort|Papa Roach|Infest|199,807|59,632|playing",
            "Music",
        )
        .expect("should parse comma decimals");
        assert_eq!(snap.query.unwrap().duration, Some(199));
        assert!((snap.sync.video_time - 59.632).abs() < 1e-6);
    }

    #[test]
    fn paused_state_sets_paused() {
        let snap = parse_now_playing("Song|Artist|Album|100|3.0|paused", "Music").unwrap();
        assert!(snap.sync.paused);
    }

    #[test]
    fn empty_output_is_none() {
        // The guarded script returns "" when nothing is playing / app is closed.
        assert!(parse_now_playing("", "Music").is_none());
        assert!(parse_now_playing("   \n", "Spotify").is_none());
    }

    #[test]
    fn short_line_is_none() {
        assert!(parse_now_playing("Song|Artist", "Music").is_none());
    }

    #[test]
    fn title_with_spaces_preserved() {
        let snap = parse_now_playing(
            "Some Long Title|The Artist|An Album|200|1.0|playing",
            "Music",
        )
        .unwrap();
        assert_eq!(snap.query.unwrap().title, "Some Long Title");
    }

    #[test]
    fn missing_artist_falls_through_to_title() {
        // Empty artist field → build_query tries to split "Artist - Title" from
        // the title; with no separator it keeps the title and a blank artist.
        let snap = parse_now_playing("Untitled||Album|10|0.0|playing", "Music").unwrap();
        let q = snap.query.unwrap();
        assert_eq!(q.artist, "");
        assert_eq!(q.title, "Untitled");
    }

    #[test]
    fn script_is_guarded_against_autolaunch() {
        let s = now_playing_script("Music");
        assert!(s.contains("is running"));
        assert!(s.contains("player state is not stopped"));
    }
}
