use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{canvas, column, container, text};
use iced::{Color, Element, Subscription, Task};
use iced_layershell::to_layer_message;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use beat::SharedClock;
use media::player::{self, PlayerEvent};
use media::{TimelineSync, TrackQuery, UltraStarSong};

mod config;
mod meter;
mod tray;
use config::{MetronomeConfig, Source};

/// App name: Wayland namespace, single-instance lock, config + data dir.
const APP: &str = "metronome";
/// Overlay accent for the lit beat / text.
const ACCENT: Color = Color::from_rgb(0.30, 0.80, 0.95);
/// Detection confidence below which an estimate is ignored.
const DETECT_MIN_CONFIDENCE: f64 = 0.15;

/// Process-global beat clock, shared by the UI, the click thread, and the
/// detector. Initialised once from the saved config.
fn clock() -> &'static SharedClock {
    static CLOCK: OnceLock<SharedClock> = OnceLock::new();
    CLOCK.get_or_init(|| {
        let cfg: MetronomeConfig = overlay::load_config(APP);
        let c = SharedClock::new(cfg.bpm, cfg.beats_per_bar);
        c.set_audible(cfg.audible);
        c.set_running(cfg.running);
        c
    })
}

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    /// ~30 fps tick advancing the visual beat flash while running.
    VisualTick,
    SetEnabled(bool),
    SetRunning(bool),
    SetSource(Source),
    /// Nudge the manual tempo by ±BPM.
    AdjustBpm(f64),
    /// Register a tap for tap-tempo.
    Tap,
    SetBeatsPerBar(u32),
    SetAudible(bool),
    SetFlash(bool),
    /// A (possibly new) track: resolve it against the library and sample position.
    TrackChanged(Option<TrackQuery>, TimelineSync),
    /// Position-only update for the current track.
    SyncUpdate(TimelineSync),
    /// A live tempo estimate from the system-audio monitor.
    BeatDetected(beat::BpmEstimate),
    /// Shift the current song's phase offset by ±ms.
    NudgeOffset(i64),
    /// Drop the current song's phase offset.
    ClearOffset,
    /// Rewrite the config with defaults and rebuild from it (tray "Restaurar padrões").
    ResetDefaults,
}

struct State {
    enabled: bool,
    running: bool,
    source: Source,
    beats_per_bar: u32,
    audible: bool,
    flash: bool,
    /// Per-song phase offsets (ms), keyed by `TrackQuery::key()`.
    offsets: std::collections::HashMap<String, i64>,
    /// Current displayed tempo (may be song-/detect-driven).
    bpm: f64,
    /// The user's manual tempo, restored when switching back to Manual.
    manual_bpm: f64,
    clock: SharedClock,
    library: Vec<media::LibraryEntry>,
    song: Option<UltraStarSong>,
    track_key: Option<String>,
    timeline_sync: Option<TimelineSync>,
    paused: bool,
    /// Recent tap timestamps for tap-tempo.
    taps: Vec<Instant>,
    /// Current beat index within the bar + its flash brightness (visual only).
    active_beat: usize,
    pulse: f32,
}

impl Default for State {
    fn default() -> Self {
        let cfg: MetronomeConfig = overlay::load_or_seed(APP);
        let library = overlay::data_dir(APP)
            .map(|d| media::library::scan(&d))
            .unwrap_or_default();
        State {
            enabled: cfg.enabled,
            running: cfg.running,
            source: Source::from_idx(cfg.source_idx),
            beats_per_bar: cfg.beats_per_bar,
            audible: cfg.audible,
            flash: cfg.flash,
            offsets: cfg.offsets,
            bpm: cfg.bpm,
            manual_bpm: cfg.bpm,
            clock: clock().clone(),
            library,
            song: None,
            track_key: None,
            timeline_sync: None,
            paused: false,
            taps: Vec::new(),
            active_beat: 0,
            pulse: 0.0,
        }
    }
}

impl State {
    /// Apply edited settings *in place*, preserving the live now-playing session
    /// (matched song, sync, taps) — unlike a full `State::default()` rebuild. Pushes
    /// the settings onto the process-global `SharedClock` (which `State` only holds a
    /// clone of, so a rebuild wouldn't re-sync it) the same way the per-message
    /// handlers do.
    fn apply_config(&mut self, cfg: MetronomeConfig) {
        self.enabled = cfg.enabled;
        self.running = cfg.running;
        self.beats_per_bar = cfg.beats_per_bar;
        self.audible = cfg.audible;
        self.flash = cfg.flash;
        self.offsets = cfg.offsets;
        self.source = Source::from_idx(cfg.source_idx);
        self.bpm = cfg.bpm;
        self.manual_bpm = cfg.bpm;
        self.clock.set_beats_per_bar(self.beats_per_bar);
        self.clock.set_audible(self.audible);
        self.clock.set_running(self.running);
        match self.source {
            Source::Manual => self.clock.set_bpm(self.manual_bpm),
            Source::Song => self.sync_to_song(),
            Source::Detect => {}
        }
        if self.running && self.source != Source::Song {
            self.clock.anchor_to(Instant::now());
        }
    }

    fn persist(&self) {
        overlay::save(
            APP,
            &MetronomeConfig {
                enabled: self.enabled,
                running: self.running,
                bpm: self.manual_bpm,
                beats_per_bar: self.beats_per_bar,
                source_idx: self.source.index(),
                audible: self.audible,
                flash: self.flash,
                offsets: self.offsets.clone(),
            },
        );
    }

    /// The current song's phase offset in ms (0 if none).
    fn current_offset(&self) -> i64 {
        self.track_key
            .as_ref()
            .and_then(|k| self.offsets.get(k))
            .copied()
            .unwrap_or(0)
    }

    /// Lock the clock onto the matched song's `#BPM`/`#GAP` grid at the current
    /// playback position (plus the per-song offset). No-op without a song+sync.
    fn sync_to_song(&mut self) {
        let (Some(song), Some(sync)) = (&self.song, &self.timeline_sync) else {
            return;
        };
        let t = sync.current_time() + self.current_offset() as f64 / 1000.0;
        let beats = song_beats(t, song.bpm, song.gap_ms / 1000.0);
        self.clock.set_bpm(song.bpm);
        self.clock.set_beats_per_bar(self.beats_per_bar);
        self.clock.rephase(Instant::now(), beats);
        self.bpm = song.bpm;
    }
}

/// Fractional beats from the song's first downbeat (`#GAP`) to time `t_secs`.
fn song_beats(t_secs: f64, bpm: f64, gap_secs: f64) -> f64 {
    (t_secs - gap_secs) * bpm / 60.0
}

impl overlay::OverlayApp for State {
    type Message = Message;

    fn namespace() -> &'static str {
        APP
    }

    fn margin_changed(margin: (i32, i32, i32, i32)) -> Message {
        Message::MarginChange(margin)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        update(self, message);
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        view(self)
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![Subscription::run(event_stream)];
        if self.enabled && self.running && self.flash {
            subs.push(Subscription::run(tick_stream));
        }
        if self.enabled && self.source == Source::Detect {
            subs.push(Subscription::run(detect_stream));
        }
        Subscription::batch(subs)
    }
}

fn update(state: &mut State, message: Message) {
    let now = Instant::now();
    match message {
        Message::VisualTick => {
            let pos = state.clock.beat_position_at(now);
            if pos >= 0.0 {
                let idx = pos.floor() as i64;
                let frac = (pos - idx as f64) as f32;
                let bar = state.beats_per_bar.max(1) as i64;
                state.active_beat = idx.rem_euclid(bar) as usize;
                state.pulse = (1.0 - frac) * (1.0 - frac);
            }
        }
        Message::SetEnabled(on) => {
            state.enabled = on;
            state.persist();
        }
        Message::SetRunning(on) => {
            state.running = on;
            state.clock.set_running(on);
            if on {
                if state.source == Source::Song {
                    state.sync_to_song();
                } else {
                    state.clock.anchor_to(now);
                }
            }
            state.persist();
        }
        Message::SetSource(s) => {
            state.source = s;
            match s {
                Source::Manual => {
                    state.clock.set_bpm(state.manual_bpm);
                    state.bpm = state.manual_bpm;
                }
                Source::Song => state.sync_to_song(),
                Source::Detect => {}
            }
            state.persist();
        }
        Message::AdjustBpm(delta) => {
            let bpm = (state.clock.bpm() + delta).clamp(beat::MIN_BPM, beat::MAX_BPM);
            state.clock.set_bpm(bpm);
            state.bpm = bpm;
            state.manual_bpm = bpm;
            state.persist();
        }
        Message::Tap => {
            state
                .taps
                .retain(|&t| now.duration_since(t) < Duration::from_secs(3));
            state.taps.push(now);
            let intervals: Vec<f64> = state
                .taps
                .windows(2)
                .map(|w| w[1].duration_since(w[0]).as_secs_f64())
                .collect();
            if let Some(bpm) = beat::tap_bpm(&intervals) {
                state.source = Source::Manual;
                state.manual_bpm = bpm;
                state.bpm = bpm;
                state.clock.set_bpm(bpm);
                state.clock.anchor_to(now); // the tap is a beat
            }
            state.persist();
        }
        Message::SetBeatsPerBar(n) => {
            state.beats_per_bar = n.max(1);
            state.clock.set_beats_per_bar(state.beats_per_bar);
            state.persist();
        }
        Message::SetAudible(on) => {
            state.audible = on;
            state.clock.set_audible(on);
            state.persist();
        }
        Message::SetFlash(on) => {
            state.flash = on;
            state.persist();
        }
        Message::TrackChanged(query, sync) => {
            state.paused = sync.paused;
            state.timeline_sync = Some(sync);
            state.track_key = query.as_ref().map(|q| q.key());
            state.song = query
                .as_ref()
                .and_then(|q| media::library::match_track(&state.library, q))
                .and_then(media::library::load)
                .and_then(media::library::Song::into_ultrastar);
            if state.source == Source::Song {
                state.sync_to_song();
            }
        }
        Message::SyncUpdate(sync) => {
            state.paused = sync.paused;
            state.timeline_sync = Some(sync);
            if state.source == Source::Song {
                state.sync_to_song();
            }
        }
        Message::BeatDetected(est) => {
            if state.source == Source::Detect && est.confidence > DETECT_MIN_CONFIDENCE {
                state.clock.set_bpm(est.bpm);
                state.bpm = est.bpm;
            }
        }
        Message::NudgeOffset(delta) => {
            if let Some(key) = state.track_key.clone() {
                let entry = state.offsets.entry(key.clone()).or_insert(0);
                *entry += delta;
                if *entry == 0 {
                    state.offsets.remove(&key);
                }
                state.persist();
                if state.source == Source::Song {
                    state.sync_to_song();
                }
            }
        }
        Message::ClearOffset => {
            if let Some(key) = &state.track_key {
                state.offsets.remove(key);
                state.persist();
                if state.source == Source::Song {
                    state.sync_to_song();
                }
            }
        }
        Message::ResetDefaults => {
            state.apply_config(overlay::reset_defaults(APP));
        }
        _ => {}
    }
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

    let header = if state.source == Source::Song && state.song.is_none() {
        format!(
            "{:.0} BPM · {} (sem música)",
            state.bpm,
            state.source.label()
        )
    } else {
        format!("{:.0} BPM · {}", state.bpm, state.source.label())
    };

    let dots = canvas(meter::Beats {
        beats_per_bar: state.beats_per_bar,
        active: state.active_beat,
        pulse: if state.running { state.pulse } else { 0.0 },
        color: ACCENT,
    })
    .width(iced::Fill)
    .height(iced::Length::Fixed(26.0));

    let mut body = column![text(header).size(18.0).color(Color::WHITE), dots,]
        .align_x(iced::Center)
        .spacing(4);

    let offset = state.current_offset();
    if state.source == Source::Song && offset != 0 {
        body = body.push(
            text(format!("⏱ {offset:+} ms"))
                .size(13.0)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.6)),
        );
    }

    container(
        container(body)
            .padding([6, 18])
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    0.0, 0.0, 0.0, 0.45,
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
    let cfg: MetronomeConfig = overlay::load_config(APP);

    ksni::TrayService::new(tray::MetronomeTray {
        tx: tx.clone(),
        enabled: cfg.enabled,
        running: cfg.running,
        audible: cfg.audible,
        flash: cfg.flash,
        source: Source::from_idx(cfg.source_idx),
        bpm: cfg.bpm,
        beats_per_bar: cfg.beats_per_bar,
    })
    .spawn();

    // Now-playing tracking (drives the Song source).
    let player_tx = tx.clone();
    std::thread::spawn(move || {
        player::run(|ev| match ev {
            PlayerEvent::Track { query, sync, .. } => player_tx
                .unbounded_send(Message::TrackChanged(query, sync))
                .is_ok(),
            PlayerEvent::Sync(sync) => player_tx.unbounded_send(Message::SyncUpdate(sync)).is_ok(),
        })
    });

    // Audio click output; self-gates on the shared clock's running && audible.
    let clock = clock().clone();
    std::thread::spawn(move || beat::run_click(clock));

    Box::pin(rx)
}

/// Gated subscription: live tempo detection runs only while the Detect source is
/// selected. Dropping this stream drops `rx`, so the detector's sink fails and the
/// capture thread exits.
fn detect_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    std::thread::spawn(move || {
        beat::run_detect(|est| tx.unbounded_send(Message::BeatDetected(est)).is_ok());
    });
    Box::pin(rx)
}

fn tick_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(33));
            if tx.unbounded_send(Message::VisualTick).is_err() {
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

    #[test]
    fn song_beats_counts_from_gap() {
        // 120 BPM, GAP 1 s: at t=2 s we're 1 s = 2 beats past the first downbeat.
        assert!((song_beats(2.0, 120.0, 1.0) - 2.0).abs() < 1e-9);
        // Exactly at GAP → beat 0 (a downbeat).
        assert!(song_beats(1.0, 120.0, 1.0).abs() < 1e-9);
        // Before GAP → negative (no clicks yet).
        assert!(song_beats(0.5, 120.0, 1.0) < 0.0);
    }
}
