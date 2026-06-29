use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{canvas, column, container, text};
use iced::{Color, Element, Subscription, Task};
use iced_layershell::to_layer_message;
use media::player::{self, PlayerEvent};
use media::{TimelineSync, TrackQuery, UltraStarSong};
use std::path::PathBuf;

mod config;
mod lane;
mod tray;
use config::KaraokeConfig;
use lane::{AHEAD_SECS, Bar, Lane, PAST_SECS};

/// App name: Wayland namespace, single-instance lock, config dir.
const APP: &str = "karaoke";
/// Panel height (taller than the thin lyrics/tuner strips).
const SURFACE_H: u32 = 220;

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    SetEnabled(bool),
    SetOffset(f64),
    /// A (possibly new) track: resolve it against the library and sample position.
    TrackChanged(Option<TrackQuery>, TimelineSync),
    /// Position-only update for the current track.
    SyncUpdate(TimelineSync),
    /// Latest detected pitch from the microphone.
    PitchUpdate(Option<pitch::Note>),
    /// Render tick to advance the scroll while playing.
    Tick,
    /// Rewrite the config with defaults and rebuild from it (tray "Restaurar padrões").
    ResetDefaults,
}

struct State {
    enabled: bool,
    offset_ms: f64,
    library_dir: Option<PathBuf>,
    library: Vec<media::LibraryEntry>,
    song: Option<UltraStarSong>,
    /// Visible MIDI range for the matched song.
    range: (f64, f64),
    timeline_sync: Option<TimelineSync>,
    paused: bool,
    /// Latest mic pitch (Phase 2 feedback cursor).
    current_note: Option<pitch::Note>,
}

impl Default for State {
    fn default() -> Self {
        let cfg: KaraokeConfig = overlay::load_or_seed(APP);
        let library_dir = cfg.library_dir.clone();
        let dir = library_dir.clone().or_else(|| overlay::data_dir(APP));
        let library = dir.map(|d| media::library::scan(&d)).unwrap_or_default();
        State {
            enabled: cfg.enabled,
            offset_ms: cfg.offset_ms,
            library_dir,
            library,
            song: None,
            range: (60.0, 72.0),
            timeline_sync: None,
            paused: false,
            current_note: None,
        }
    }
}

impl State {
    /// Apply edited settings *in place*, preserving the live now-playing session
    /// (matched song, sync) — unlike a full `State::default()` rebuild, which would
    /// blank the lane until the next player event. Rescans the library only when
    /// its directory actually changed, so a routine edit never blocks the UI thread
    /// on a directory walk.
    fn apply_config(&mut self, cfg: KaraokeConfig) {
        self.enabled = cfg.enabled;
        self.offset_ms = cfg.offset_ms;
        if cfg.library_dir != self.library_dir {
            self.library_dir = cfg.library_dir.clone();
            let dir = self.library_dir.clone().or_else(|| overlay::data_dir(APP));
            self.library = dir.map(|d| media::library::scan(&d)).unwrap_or_default();
        }
    }

    fn persist(&self) {
        overlay::save(
            APP,
            &KaraokeConfig {
                enabled: self.enabled,
                library_dir: self.library_dir.clone(),
                offset_ms: self.offset_ms,
            },
        );
    }

    /// Current playback position with the manual offset applied.
    fn current_time(&self) -> Option<f64> {
        self.timeline_sync
            .as_ref()
            .map(|s| s.current_time() + self.offset_ms / 1000.0)
    }
}

/// Padded MIDI span of a song's notes (a couple of semitones of headroom).
fn midi_range(song: &UltraStarSong) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for n in &song.notes {
        lo = lo.min(n.midi);
        hi = hi.max(n.midi);
    }
    if lo.is_finite() {
        (lo - 2.0, hi + 2.0)
    } else {
        (60.0, 72.0)
    }
}

/// Shift `v` by whole octaves until it lands within a semitone-tritone of
/// `target`, so octave-off singing still reads as on-pitch (forgiving feedback).
fn octave_fold(mut v: f64, target: f64) -> f64 {
    while v - target > 6.0 {
        v -= 12.0;
    }
    while target - v > 6.0 {
        v += 12.0;
    }
    v
}

/// Signed cents of the (octave-folded, continuous) sung pitch from `target`.
/// `sung_pitch` is a continuous MIDI value (`note.midi + note.cents/100`), so the
/// feedback keeps fractional precision instead of snapping to whole semitones.
fn cents_to_target(sung_pitch: f64, target: f64) -> f64 {
    (octave_fold(sung_pitch, target) - target) * 100.0
}

/// The syllables of the phrase active at time `t`, joined (UltraStar syllables
/// carry their own spacing, so concatenation reproduces the words).
fn current_line(song: &UltraStarSong, t: f64) -> String {
    let start = song
        .breaks
        .iter()
        .copied()
        .filter(|&b| b <= t)
        .fold(0.0, f64::max);
    let end = song
        .breaks
        .iter()
        .copied()
        .find(|&b| b > t)
        .unwrap_or(f64::INFINITY);
    let mut s = String::new();
    for n in &song.notes {
        if n.start >= start && n.start < end {
            s.push_str(&n.text);
        }
    }
    s.trim().to_string()
}

impl overlay::OverlayApp for State {
    type Message = Message;

    fn namespace() -> &'static str {
        APP
    }

    fn margin_changed(margin: (i32, i32, i32, i32)) -> Self::Message {
        Message::MarginChange(margin)
    }

    // A large fixed panel: own the geometry, opt out of the bottom-strip stacking.
    fn surface_height() -> u32 {
        SURFACE_H
    }
    fn stacks() -> bool {
        false
    }
    fn initial_margin() -> (i32, i32, i32, i32) {
        (0, 0, 40, 0)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        update(self, message)
    }
    fn view(&self) -> Element<'_, Message> {
        view(self)
    }
    fn subscription(&self) -> Subscription<Message> {
        let events = Subscription::run(event_stream);
        // Advance the scroll only while a matched song is actually playing.
        let needs_tick = self.enabled && self.song.is_some() && !self.paused;
        if needs_tick {
            Subscription::batch([events, Subscription::run(tick_stream)])
        } else {
            events
        }
    }
}

fn update(state: &mut State, msg: Message) -> Task<Message> {
    match msg {
        Message::SetEnabled(e) => {
            state.enabled = e;
            state.persist();
        }
        Message::SetOffset(o) => {
            state.offset_ms = o;
            state.persist();
        }
        Message::TrackChanged(query, sync) => {
            state.paused = sync.paused;
            state.timeline_sync = Some(sync);
            state.song = query
                .as_ref()
                .and_then(|q| media::library::match_track(&state.library, q))
                .and_then(media::library::load)
                .and_then(media::library::Song::into_ultrastar);
            if let Some(song) = &state.song {
                state.range = midi_range(song);
            }
        }
        Message::SyncUpdate(sync) => {
            state.paused = sync.paused;
            state.timeline_sync = Some(sync);
        }
        Message::PitchUpdate(n) => state.current_note = n,
        Message::Tick => {}
        Message::ResetDefaults => {
            state.apply_config(overlay::reset_defaults(APP));
        }
        _ => {}
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let empty = || {
        container(text(""))
            .center_x(iced::Fill)
            .center_y(iced::Fill)
    };
    if !state.enabled {
        return empty().into();
    }
    let (Some(song), Some(t)) = (&state.song, state.current_time()) else {
        return empty().into();
    };

    let (lo, hi) = state.range;
    let (win_start, win_end) = (t - PAST_SECS, t + AHEAD_SECS);
    let bars: Vec<Bar> = song
        .notes
        .iter()
        .filter(|n| n.end >= win_start && n.start <= win_end)
        .map(|n| {
            let (name, octave) = pitch::midi_name(n.midi as i64);
            Bar {
                start: n.start,
                end: n.end,
                midi: n.midi,
                golden: n.golden,
                name,
                octave,
            }
        })
        .collect();

    // The note you're meant to sing right now (the bar under the playhead).
    let active_target = song
        .notes
        .iter()
        .find(|n| n.start <= t && t < n.end)
        .map(|n| n.midi);

    // Your sung pitch as a *continuous* MIDI value, so feedback keeps cents
    // precision (the old code used the rounded note.midi, quantizing to semitones).
    let sung_pitch = state.current_note.map(|n| n.midi + n.cents / 100.0);
    let (sung, cursor_color) = match (sung_pitch, active_target) {
        (Some(p), Some(tgt)) => {
            let [r, g, b] = pitch::cents_color(cents_to_target(p, tgt));
            (Some(octave_fold(p, tgt)), Color::from_rgb(r, g, b))
        }
        (Some(p), None) => (
            Some(octave_fold(p, (lo + hi) / 2.0)),
            Color::from_rgba(0.85, 0.85, 0.85, 0.9),
        ),
        (None, _) => (None, Color::from_rgba(1.0, 1.0, 1.0, 0.6)),
    };

    // Readout: the target note to sing, and the note you're singing (colored
    // green when you match the target). Works even between notes, so it doubles
    // as a mic check.
    let target_label = match active_target {
        Some(m) => {
            let (n, o) = pitch::midi_name(m as i64);
            format!("Cante: {n}{o}")
        }
        None => "Cante: —".to_string(),
    };
    let (you_label, you_color) = match state.current_note {
        Some(note) => (
            format!("Você: {}{} {:+.0}¢", note.name, note.octave, note.cents),
            cursor_color,
        ),
        None => (
            "Você: — (microfone?)".to_string(),
            Color::from_rgba(1.0, 1.0, 1.0, 0.6),
        ),
    };
    let readout = iced::widget::row![
        text(target_label).size(18.0).color(Color::WHITE),
        text(you_label).size(18.0).color(you_color),
    ]
    .spacing(28);

    let lane = canvas(Lane {
        bars,
        t,
        lo,
        hi,
        sung,
        cursor_color,
    })
    .width(iced::Fill)
    .height(iced::Length::Fixed(120.0));

    let body = column![
        readout,
        lane,
        text(current_line(song, t)).size(22.0).color(Color::WHITE),
    ]
    .align_x(iced::Center)
    .spacing(4);

    container(
        container(body)
            .padding([8, 16])
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    0.0, 0.0, 0.0, 0.55,
                ))),
                border: iced::Border {
                    radius: 12.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
    )
    .center_x(iced::Fill)
    .center_y(iced::Fill)
    .into()
}

fn event_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    let cfg: KaraokeConfig = overlay::load_config(APP);

    ksni::TrayService::new(tray::KaraokeTray {
        tx: tx.clone(),
        enabled: cfg.enabled,
        offset_ms: cfg.offset_ms,
    })
    .spawn();

    let player_tx = tx.clone();
    std::thread::spawn(move || {
        player::run(|ev| match ev {
            PlayerEvent::Track { query, sync, .. } => player_tx
                .unbounded_send(Message::TrackChanged(query, sync))
                .is_ok(),
            PlayerEvent::Sync(sync) => player_tx.unbounded_send(Message::SyncUpdate(sync)).is_ok(),
        })
    });

    // Own microphone stream (no IPC with the tuner process), always-on like tuner.
    std::thread::spawn(move || {
        pitch::run_capture(|freq, _level| {
            let note = freq.map(|f| pitch::frequency_to_note(f, pitch::A4));
            tx.unbounded_send(Message::PitchUpdate(note)).is_ok()
        });
    });

    Box::pin(rx)
}

fn tick_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(33));
            if tx.unbounded_send(Message::Tick).is_err() {
                break;
            }
        }
    });
    Box::pin(rx)
}

fn main() -> iced_layershell::Result {
    overlay::run::<State>()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SONG: &str = "#TITLE:T\n#ARTIST:A\n#BPM:120\n#GAP:0\n\
: 0 4 0 Hel\n: 4 4 0 lo\n- 8\n: 8 4 12 World\nE\n";

    fn song() -> UltraStarSong {
        media::ultrastar::parse_ultrastar(SONG).unwrap()
    }

    #[test]
    fn midi_range_pads_extremes() {
        let (lo, hi) = midi_range(&song());
        // pitches 0 and 12 -> midi 60 and 72, padded by 2.
        assert_eq!((lo, hi), (58.0, 74.0));
    }

    #[test]
    fn cents_to_target_is_continuous_not_quantized() {
        // The bug: feeding a rounded MIDI quantized this to multiples of 100¢.
        // Singing 30 cents sharp of the target must read ~+30¢, not 0.
        assert!((cents_to_target(60.30, 60.0) - 30.0).abs() < 1e-9);
        assert!(cents_to_target(60.0, 60.0).abs() < 1e-9);
        assert!((cents_to_target(59.80, 60.0) + 20.0).abs() < 1e-9);
        // Octave-off singing still reads as on-target (folded).
        assert!(cents_to_target(72.0, 60.0).abs() < 1e-9);
        assert!((cents_to_target(72.10, 60.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn octave_fold_brings_octave_off_pitch_close() {
        // Singing C5 (72) against a C4 (60) target folds to 60.
        assert_eq!(octave_fold(72.0, 60.0), 60.0);
        // C3 (48) against C4 folds up to 60.
        assert_eq!(octave_fold(48.0, 60.0), 60.0);
        // Already close: unchanged.
        assert_eq!(octave_fold(62.0, 60.0), 62.0);
    }

    #[test]
    fn current_line_groups_by_break() {
        let s = song();
        // beat_ms = 125ms; break at beat 8 -> 1.0s. First line before 1.0s.
        assert_eq!(current_line(&s, 0.1), "Hello");
        assert_eq!(current_line(&s, 1.2), "World");
    }
}
