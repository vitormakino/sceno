//! MPRIS (D-Bus) player tracking.
//!
//! Polls the active media player for now-playing metadata and position, fetches
//! synced lyrics from LRCLIB on track changes, and feeds the existing timeline
//! machinery via `Message::CuesReceived` / `Message::SyncReceived`.

use crate::{lrc, lrclib, Message, TimelineSync};
use futures::channel::mpsc::UnboundedSender;
use lrclib::TrackQuery;
use mpris::{PlaybackStatus, PlayerFinder};
use std::time::{Duration, Instant};

/// How often to re-anchor position from MPRIS while a track is playing.
const POLL_INTERVAL: Duration = Duration::from_millis(1000);
/// Backoff when no player is present.
const IDLE_INTERVAL: Duration = Duration::from_secs(2);

/// Build a lyrics query from raw MPRIS metadata fields. When the player exposes
/// no artist (common for browser tabs), fall back to splitting an
/// "Artist - Title" style title. Returns `None` if there's no usable title.
pub fn build_query(
    title: Option<String>,
    artists: Vec<String>,
    album: Option<String>,
    length_secs: Option<u64>,
) -> Option<TrackQuery> {
    let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())?;

    let artist = artists
        .into_iter()
        .map(|a| a.trim().to_string())
        .find(|a| !a.is_empty());

    let (artist, title) = match artist {
        Some(a) => (a, title),
        None => match title.split_once(" - ") {
            Some((a, t)) if !a.trim().is_empty() && !t.trim().is_empty() => {
                (a.trim().to_string(), t.trim().to_string())
            }
            _ => (String::new(), title),
        },
    };

    Some(TrackQuery {
        artist,
        title,
        album: album.map(|a| a.trim().to_string()).filter(|a| !a.is_empty()),
        duration: length_secs.filter(|&d| d > 0),
    })
}

/// Run the MPRIS polling loop forever (intended to own a dedicated thread).
pub fn run(tx: UnboundedSender<Message>) {
    loop {
        if let Ok(finder) = PlayerFinder::new() {
            track_active_player(&finder, &tx);
        }
        // Either no D-Bus or the player vanished — back off and retry.
        std::thread::sleep(IDLE_INTERVAL);
    }
}

/// Follow the active player until it disappears or errors, then return so the
/// caller can re-discover.
fn track_active_player(finder: &PlayerFinder, tx: &UnboundedSender<Message>) {
    let mut current_key: Option<String> = None;

    loop {
        let Ok(player) = finder.find_active() else { return };
        let Ok(metadata) = player.get_metadata() else { return };

        let query = build_query(
            metadata.title().map(str::to_string),
            metadata
                .artists()
                .map(|a| a.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default(),
            metadata.album_name().map(str::to_string),
            metadata.length().map(|d| d.as_secs()),
        );

        let paused = !matches!(
            player.get_playback_status().unwrap_or(PlaybackStatus::Stopped),
            PlaybackStatus::Playing
        );
        let position = player.get_position().unwrap_or(Duration::ZERO).as_secs_f64();
        let rate = player.get_playback_rate().unwrap_or(1.0);
        let sync = TimelineSync {
            video_time: position,
            captured_at: Instant::now(),
            paused,
            playback_rate: if rate > 0.0 { rate } else { 1.0 },
        };

        let key = query.as_ref().map(TrackQuery::key);
        let send_result = if key != current_key {
            current_key = key;
            // Track changed (or first sight) — fetch lyrics and reset the timeline.
            let cues = query
                .as_ref()
                .and_then(lrclib::fetch_synced)
                .map(|s| lrc::parse_lrc(&s))
                .unwrap_or_default();
            tx.unbounded_send(Message::CuesReceived(cues, sync))
        } else {
            tx.unbounded_send(Message::SyncReceived(sync))
        };

        if send_result.is_err() {
            return; // app shutting down
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_query_uses_explicit_artist() {
        let q = build_query(
            Some("Dreams".into()),
            vec!["Fleetwood Mac".into()],
            Some("Rumours".into()),
            Some(257),
        )
        .unwrap();
        assert_eq!(q.artist, "Fleetwood Mac");
        assert_eq!(q.title, "Dreams");
        assert_eq!(q.album.as_deref(), Some("Rumours"));
        assert_eq!(q.duration, Some(257));
    }

    #[test]
    fn build_query_splits_artist_from_title_when_missing() {
        let q = build_query(Some("Daft Punk - Get Lucky".into()), vec![], None, None).unwrap();
        assert_eq!(q.artist, "Daft Punk");
        assert_eq!(q.title, "Get Lucky");
    }

    #[test]
    fn build_query_keeps_title_when_no_separator() {
        let q = build_query(Some("Untitled".into()), vec![], None, None).unwrap();
        assert_eq!(q.artist, "");
        assert_eq!(q.title, "Untitled");
    }

    #[test]
    fn build_query_ignores_blank_artists() {
        let q = build_query(Some("Song".into()), vec!["   ".into()], None, None).unwrap();
        // blank artist falls through to title-splitting, which has no separator
        assert_eq!(q.artist, "");
        assert_eq!(q.title, "Song");
    }

    #[test]
    fn build_query_none_without_title() {
        assert!(build_query(None, vec!["Artist".into()], None, None).is_none());
        assert!(build_query(Some("  ".into()), vec![], None, None).is_none());
    }

    #[test]
    fn build_query_drops_zero_duration_and_empty_album() {
        let q = build_query(Some("S".into()), vec!["A".into()], Some("".into()), Some(0)).unwrap();
        assert_eq!(q.album, None);
        assert_eq!(q.duration, None);
    }
}
