use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{column, container, row, text};
use iced::{Color, Element, Subscription, Task};
use iced_layershell::to_layer_message;
use media::player::{self, PlayerEvent};
use media::{CueEntry, TimelineSync, TrackQuery, cue_at, lines_at};
use overlay::FontSize;
use std::collections::HashMap;

mod config;
use config::SavedConfig;

/// App name: used for the Wayland namespace, the single-instance lock, and the
/// config/cache directory (`~/.config/sceno/lyrics`, `~/.cache/sceno/lyrics`).
const APP: &str = "lyrics";

/// One nudge step (ms) for the per-song sync offset.
const NUDGE_MS: f64 = 100.0;

/// How long (seconds, from track start) to announce the now-playing title while
/// no lyric line is active — a heads-up of what's about to play.
const ANNOUNCE_SECS: f64 = 5.0;

/// Color of not-yet-sung words on the active line (dim white).
const UNSUNG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.45);
/// Color of the dimmed lookahead (next) line.
const LOOKAHEAD: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.5);

// ── App types ─────────────────────────────────────────────────────────────────

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    SetEnabled(bool),
    SetFontSize(FontSize),
    TrackReceived(Option<TrackQuery>, Vec<CueEntry>, TimelineSync),
    SyncReceived(TimelineSync),
    /// Adjust the current song's sync offset by ±`NUDGE_MS`.
    NudgeOffset(f64),
    /// Drop the current song's saved sync offset.
    ClearOffset,
    /// Toggle the dimmed lookahead (next) line.
    SetShowNext(bool),
    TimelineTick,
    /// Rewrite the config with defaults and rebuild from it (tray "Restaurar padrões").
    ResetDefaults,
}

struct State {
    caption: String,
    enabled: bool,
    font_size: FontSize,
    cues: Vec<CueEntry>,
    timeline_sync: Option<TimelineSync>,
    paused: bool,
    /// Stable key of the playing track (for offset lookup), if known.
    track_key: Option<String>,
    /// Human-readable now-playing label, used for the intro announcement.
    track_label: Option<String>,
    /// Per-song sync offsets (ms), keyed by track. Mirrors the persisted config.
    offsets: HashMap<String, f64>,
    /// Show the upcoming line dimmed below the active one (lookahead).
    show_next: bool,
}

impl Default for State {
    fn default() -> Self {
        let cfg: SavedConfig = overlay::load_or_seed(APP);
        State {
            caption: String::new(),
            enabled: cfg.enabled,
            font_size: FontSize::from_idx(cfg.font_size_idx),
            cues: Vec::new(),
            timeline_sync: None,
            paused: false,
            track_key: None,
            track_label: None,
            offsets: cfg.offsets,
            show_next: cfg.show_next,
        }
    }
}

impl State {
    /// The current song's sync offset in milliseconds (`0.0` if none saved).
    fn current_offset_ms(&self) -> f64 {
        self.track_key
            .as_ref()
            .and_then(|k| self.offsets.get(k))
            .copied()
            .unwrap_or(0.0)
    }

    /// Offset-adjusted playback time (seconds), if a sync is known. This is the
    /// time line/word lookups are anchored to, so the per-song nudge shifts the
    /// line selection and the word-by-word fill together.
    fn adjusted_time(&self) -> Option<f64> {
        self.timeline_sync
            .as_ref()
            .map(|s| s.current_time() + self.current_offset_ms() / 1000.0)
    }

    /// Persist font size, enabled, the per-song offset map, and lookahead toggle.
    fn persist(&self) {
        overlay::save(
            APP,
            &SavedConfig {
                font_size_idx: self.font_size.index(),
                enabled: self.enabled,
                offsets: self.offsets.clone(),
                show_next: self.show_next,
            },
        );
    }
}

/// Build the now-playing label shown during the intro announcement.
fn track_label(query: &TrackQuery) -> String {
    if query.artist.trim().is_empty() {
        format!("♪ {}", query.title)
    } else {
        format!("♪ {} — {}", query.artist, query.title)
    }
}

/// Recompute the visible caption from the current cues and sync position,
/// applying the song's sync offset. Falls back to the now-playing announcement
/// during the track's intro when no lyric line is active.
fn apply_timeline_caption(state: &mut State) {
    if let Some(t) = state.adjusted_time() {
        state.caption = if let Some(line) = cue_at(&state.cues, t) {
            line.to_string()
        } else if (0.0..ANNOUNCE_SECS).contains(&t) {
            state.track_label.clone().unwrap_or_default()
        } else {
            String::new()
        };
    }
}

// ── System tray ───────────────────────────────────────────────────────────────

struct LyricsTray {
    tx: mpsc::UnboundedSender<Message>,
    enabled: bool,
    font_size: FontSize,
    show_next: bool,
}

impl LyricsTray {
    fn nudge(&self, delta: f64) {
        let _ = self.tx.unbounded_send(Message::NudgeOffset(delta));
    }
}

impl ksni::Tray for LyricsTray {
    fn icon_name(&self) -> String {
        "audio-x-generic".into()
    }
    fn title(&self) -> String {
        "sceno · lyrics".into()
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let fs = self.font_size;
        vec![
            CheckmarkItem {
                label: "Overlay ativo".into(),
                checked: self.enabled,
                activate: Box::new(|this: &mut Self| {
                    this.enabled = !this.enabled;
                    let _ = this.tx.unbounded_send(Message::SetEnabled(this.enabled));
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Próxima linha".into(),
                checked: self.show_next,
                activate: Box::new(|this: &mut Self| {
                    this.show_next = !this.show_next;
                    let _ = this.tx.unbounded_send(Message::SetShowNext(this.show_next));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: "Tamanho da fonte".into(),
                submenu: vec![
                    RadioGroup {
                        selected: fs.index(),
                        select: Box::new(|this: &mut Self, idx| {
                            this.font_size = match idx {
                                0 => FontSize::Small,
                                1 => FontSize::Medium,
                                _ => FontSize::Large,
                            };
                            let _ = this.tx.unbounded_send(Message::SetFontSize(this.font_size));
                        }),
                        options: vec![
                            RadioItem {
                                label: "Pequeno".into(),
                                ..Default::default()
                            },
                            RadioItem {
                                label: "Médio".into(),
                                ..Default::default()
                            },
                            RadioItem {
                                label: "Grande".into(),
                                ..Default::default()
                            },
                        ],
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: "Sincronia (esta música)".into(),
                submenu: vec![
                    StandardItem {
                        label: "Adiantar legenda (+100ms)".into(),
                        activate: Box::new(|this: &mut Self| this.nudge(NUDGE_MS)),
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "Atrasar legenda (−100ms)".into(),
                        activate: Box::new(|this: &mut Self| this.nudge(-NUDGE_MS)),
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "Limpar ajuste desta música".into(),
                        activate: Box::new(|this: &mut Self| {
                            let _ = this.tx.unbounded_send(Message::ClearOffset);
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Restaurar padrões".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.unbounded_send(Message::ResetDefaults);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Sair".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

// ── OverlayApp impl ───────────────────────────────────────────────────────────

impl overlay::OverlayApp for State {
    type Message = Message;

    fn namespace() -> &'static str {
        APP
    }

    fn margin_changed(margin: (i32, i32, i32, i32)) -> Self::Message {
        Message::MarginChange(margin)
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        update(self, message)
    }

    fn view(&self) -> Element<'_, Self::Message> {
        view(self)
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let events = Subscription::run(event_stream);
        // Only tick when playing — paused playback needs no interpolation.
        let needs_tick = self.enabled && !self.cues.is_empty() && !self.paused;
        if needs_tick {
            Subscription::batch([events, Subscription::run(timeline_tick_stream)])
        } else {
            events
        }
    }
}

fn main() -> iced_layershell::Result {
    overlay::run::<State>()
}

fn update(state: &mut State, msg: Message) -> Task<Message> {
    match msg {
        Message::SetEnabled(e) => {
            state.enabled = e;
            if e {
                // Show the cue for the current position immediately.
                apply_timeline_caption(state);
            } else {
                state.caption.clear();
            }
            state.persist();
        }
        Message::SetFontSize(s) => {
            state.font_size = s;
            state.persist();
        }
        Message::TrackReceived(query, cues, sync) => {
            state.paused = sync.paused;
            state.cues = cues;
            state.timeline_sync = Some(sync);
            state.track_key = query.as_ref().map(TrackQuery::key);
            state.track_label = query.as_ref().map(track_label);
            if state.enabled {
                apply_timeline_caption(state);
            }
        }
        Message::SyncReceived(sync) => {
            state.paused = sync.paused;
            state.timeline_sync = Some(sync);
            if state.enabled {
                apply_timeline_caption(state);
            }
        }
        Message::NudgeOffset(delta) => {
            if let Some(key) = state.track_key.clone() {
                let v = state.offsets.entry(key.clone()).or_insert(0.0);
                *v += delta;
                if *v == 0.0 {
                    state.offsets.remove(&key);
                }
                state.persist();
                apply_timeline_caption(state);
            }
        }
        Message::ClearOffset => {
            if let Some(key) = &state.track_key
                && state.offsets.remove(key).is_some()
            {
                state.persist();
                apply_timeline_caption(state);
            }
        }
        Message::SetShowNext(v) => {
            state.show_next = v;
            state.persist();
        }
        Message::TimelineTick if state.enabled => {
            apply_timeline_caption(state);
        }
        Message::ResetDefaults => {
            overlay::save(APP, &SavedConfig::default());
            // Rebuild from the fresh config; transient now-playing state (caption,
            // cues, sync) re-fills on the next player event.
            *state = State::default();
        }
        _ => {}
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let offset_ms = state.current_offset_ms();
    // Only show the pill when there's something to display: a caption, or — so a
    // mid-song nudge is visible even during a lyric gap — an active offset.
    if state.caption.is_empty() && offset_ms == 0.0 {
        return container(text(""))
            .center_x(iced::Fill)
            .center_y(iced::Fill)
            .into();
    }

    let font = state.font_size.px();
    let t = state.adjusted_time();
    let lines = t.map(|t| lines_at(&state.cues, t)).unwrap_or_default();

    // Active-line widget: word-by-word fill when the cue carries word timings,
    // otherwise the whole caption bright (announcement and line-level lyrics).
    let caption: Element<'_, Message> = match (t, lines.current) {
        (Some(t), Some(cur)) if !cur.words.is_empty() && cur.text == state.caption => {
            let sung = cur.sung_words(t);
            let bright: String = cur.words[..sung].iter().map(|w| w.text.as_str()).collect();
            let dim: String = cur.words[sung..].iter().map(|w| w.text.as_str()).collect();
            let mut words = row![].spacing(0).align_y(iced::Center);
            if !bright.is_empty() {
                words = words.push(text(bright).size(font).color(Color::WHITE));
            }
            if !dim.is_empty() {
                words = words.push(text(dim).size(font).color(UNSUNG));
            }
            words.into()
        }
        _ => text(state.caption.clone())
            .size(font)
            .color(Color::WHITE)
            .into(),
    };

    // Caption + optional "+/-NNN ms" chip (evidence of the per-song customization).
    let mut head = row![caption].spacing(10).align_y(iced::Center);
    if offset_ms != 0.0 {
        head = head.push(
            text(format!("⏱ {offset_ms:+.0} ms"))
                .size((font * 0.5).max(14.0))
                .color(Color::from_rgba(1.0, 0.85, 0.4, 0.9)),
        );
    }

    // Lookahead: the next non-empty line, dimmed and smaller, when enabled.
    let mut body = column![head].spacing(2).align_x(iced::Center);
    if state.show_next
        && !state.caption.is_empty()
        && let Some(next) = lines.next
    {
        body = body.push(
            text(next.text.clone())
                .size((font * 0.6).max(13.0))
                .color(LOOKAHEAD),
        );
    }

    container(
        container(body)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    0.0, 0.0, 0.0, 0.6,
                ))),
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .padding([6, 14]),
    )
    .center_x(iced::Fill)
    .center_y(iced::Fill)
    .into()
}

// ── Subscription streams ──────────────────────────────────────────────────────

fn event_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    let cfg: SavedConfig = overlay::load_config(APP);

    ksni::TrayService::new(LyricsTray {
        tx: tx.clone(),
        enabled: cfg.enabled,
        font_size: FontSize::from_idx(cfg.font_size_idx),
        show_next: cfg.show_next,
    })
    .spawn();

    std::thread::spawn(move || {
        player::run(|ev| match ev {
            PlayerEvent::Track { query, cues, sync } => tx
                .unbounded_send(Message::TrackReceived(query, cues, sync))
                .is_ok(),
            PlayerEvent::Sync(sync) => tx.unbounded_send(Message::SyncReceived(sync)).is_ok(),
        })
    });

    Box::pin(rx)
}

fn timeline_tick_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if tx.unbounded_send(Message::TimelineTick).is_err() {
                break;
            }
        }
    });
    Box::pin(rx)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn test_state() -> State {
        State {
            caption: String::new(),
            enabled: true,
            font_size: FontSize::Medium,
            cues: Vec::new(),
            timeline_sync: None,
            paused: false,
            track_key: None,
            track_label: None,
            offsets: HashMap::new(),
            show_next: true,
        }
    }

    fn sample_query() -> TrackQuery {
        TrackQuery {
            artist: "Daft Punk".into(),
            title: "Get Lucky".into(),
            album: None,
            duration: None,
        }
    }

    fn paused_sync(t: f64) -> TimelineSync {
        TimelineSync {
            video_time: t,
            captured_at: Instant::now(),
            paused: true,
            playback_rate: 1.0,
        }
    }

    fn sample_cues() -> Vec<CueEntry> {
        vec![
            CueEntry {
                start: 1.0,
                end: 3.0,
                text: "hello".into(),
                words: Vec::new(),
            },
            CueEntry {
                start: 3.0,
                end: 5.0,
                text: "world".into(),
                words: Vec::new(),
            },
            CueEntry {
                start: 5.0,
                end: 7.0,
                text: "foo".into(),
                words: Vec::new(),
            },
        ]
    }

    // ── apply_timeline_caption ────────────────────────────────────────────────

    #[test]
    fn apply_sets_matching_cue() {
        let mut s = test_state();
        s.cues = sample_cues();
        s.timeline_sync = Some(paused_sync(2.0));
        apply_timeline_caption(&mut s);
        assert_eq!(s.caption, "hello");
    }

    #[test]
    fn apply_clears_when_no_cue_matches() {
        let mut s = test_state();
        s.caption = "stale".into();
        s.cues = sample_cues();
        s.timeline_sync = Some(paused_sync(0.0)); // antes do primeiro cue
        apply_timeline_caption(&mut s);
        assert_eq!(s.caption, "");
    }

    #[test]
    fn apply_noop_without_sync() {
        let mut s = test_state();
        s.caption = "existing".into();
        s.cues = sample_cues();
        s.timeline_sync = None;
        apply_timeline_caption(&mut s);
        assert_eq!(s.caption, "existing"); // não muda sem sync
    }

    // ── SetEnabled ────────────────────────────────────────────────────────────

    #[test]
    fn set_enabled_false_clears_caption() {
        let mut s = test_state();
        s.caption = "something".into();
        let _ = update(&mut s, Message::SetEnabled(false));
        assert_eq!(s.caption, "");
        assert!(!s.enabled);
    }

    #[test]
    fn set_enabled_true_applies_current_cue() {
        let mut s = test_state();
        s.enabled = false;
        s.cues = sample_cues();
        s.timeline_sync = Some(paused_sync(4.0));
        let _ = update(&mut s, Message::SetEnabled(true));
        assert_eq!(s.caption, "world");
        assert!(s.enabled);
    }

    #[test]
    fn set_enabled_true_no_sync_stays_empty() {
        let mut s = test_state();
        s.enabled = false;
        s.cues = sample_cues();
        s.timeline_sync = None;
        let _ = update(&mut s, Message::SetEnabled(true));
        assert_eq!(s.caption, "");
    }

    // ── CuesReceived ──────────────────────────────────────────────────────────

    #[test]
    fn track_received_updates_caption_and_paused() {
        let mut s = test_state();
        let sync = paused_sync(2.0);
        let _ = update(
            &mut s,
            Message::TrackReceived(Some(sample_query()), sample_cues(), sync),
        );
        assert_eq!(s.caption, "hello");
        assert!(s.paused);
        assert_eq!(s.track_key, Some(sample_query().key()));
    }

    #[test]
    fn track_received_silent_when_disabled() {
        let mut s = test_state();
        s.enabled = false;
        let _ = update(
            &mut s,
            Message::TrackReceived(None, sample_cues(), paused_sync(2.0)),
        );
        assert_eq!(s.caption, "");
    }

    // ── Announcement ──────────────────────────────────────────────────────────

    #[test]
    fn announces_title_during_intro_when_no_cue() {
        let mut s = test_state();
        // Intro (t=0.5): no cue active, so the now-playing label shows.
        let _ = update(
            &mut s,
            Message::TrackReceived(Some(sample_query()), sample_cues(), paused_sync(0.5)),
        );
        assert_eq!(s.caption, "♪ Daft Punk — Get Lucky");
    }

    #[test]
    fn announcement_yields_to_active_cue() {
        let mut s = test_state();
        // At t=2.0 the first cue is active and wins over the announcement.
        let _ = update(
            &mut s,
            Message::TrackReceived(Some(sample_query()), sample_cues(), paused_sync(2.0)),
        );
        assert_eq!(s.caption, "hello");
    }

    #[test]
    fn no_announcement_after_intro_window() {
        let mut s = test_state();
        s.cues = Vec::new();
        s.track_label = Some("♪ Daft Punk — Get Lucky".into());
        s.timeline_sync = Some(paused_sync(ANNOUNCE_SECS + 1.0));
        apply_timeline_caption(&mut s);
        assert_eq!(s.caption, "");
    }

    // ── Per-song offset ───────────────────────────────────────────────────────

    #[test]
    fn nudge_shifts_active_cue() {
        let mut s = test_state();
        s.track_key = Some("k".into());
        s.cues = sample_cues();
        s.timeline_sync = Some(paused_sync(2.9)); // "hello" ends at 3.0
        // Advance lyrics by 200ms → t becomes 3.1, landing in "world".
        let _ = update(&mut s, Message::NudgeOffset(200.0));
        assert_eq!(s.offsets.get("k"), Some(&200.0));
        assert_eq!(s.caption, "world");
    }

    #[test]
    fn nudge_back_to_zero_removes_entry() {
        let mut s = test_state();
        s.track_key = Some("k".into());
        let _ = update(&mut s, Message::NudgeOffset(100.0));
        let _ = update(&mut s, Message::NudgeOffset(-100.0));
        assert!(!s.offsets.contains_key("k"));
        assert_eq!(s.current_offset_ms(), 0.0);
    }

    #[test]
    fn nudge_without_track_is_noop() {
        let mut s = test_state();
        s.track_key = None;
        let _ = update(&mut s, Message::NudgeOffset(100.0));
        assert!(s.offsets.is_empty());
    }

    #[test]
    fn clear_offset_drops_saved_entry() {
        let mut s = test_state();
        s.track_key = Some("k".into());
        s.offsets.insert("k".into(), 300.0);
        let _ = update(&mut s, Message::ClearOffset);
        assert!(!s.offsets.contains_key("k"));
    }

    #[test]
    fn adjusted_time_applies_offset() {
        let mut s = test_state();
        s.track_key = Some("k".into());
        s.timeline_sync = Some(paused_sync(10.0));
        assert_eq!(s.adjusted_time(), Some(10.0));
        // +250ms nudge advances the anchor time the word fill reads from.
        s.offsets.insert("k".into(), 250.0);
        assert_eq!(s.adjusted_time(), Some(10.25));
    }

    #[test]
    fn adjusted_time_none_without_sync() {
        let s = test_state();
        assert_eq!(s.adjusted_time(), None);
    }

    #[test]
    fn word_cue_fill_tracks_adjusted_time() {
        // A word-timed line: at the offset-adjusted time, sung_words reflects how
        // many words the fill should brighten.
        let cue = CueEntry {
            start: 1.0,
            end: 5.0,
            text: "hello world".into(),
            words: vec![
                media::WordTiming {
                    start: 1.0,
                    text: "hello ".into(),
                },
                media::WordTiming {
                    start: 3.0,
                    text: "world".into(),
                },
            ],
        };
        let mut s = test_state();
        s.track_key = Some("k".into());
        s.cues = vec![cue];
        s.timeline_sync = Some(paused_sync(2.0)); // between the two word onsets
        let t = s.adjusted_time().unwrap();
        assert_eq!(s.cues[0].sung_words(t), 1);
        // Nudge forward 1.5s → t=3.5, both words now sung.
        s.offsets.insert("k".into(), 1500.0);
        let t = s.adjusted_time().unwrap();
        assert_eq!(s.cues[0].sung_words(t), 2);
    }

    #[test]
    fn offset_persists_across_track_switch() {
        let mut s = test_state();
        s.offsets.insert("k".into(), 150.0);
        // A different track has no offset; switching back restores 150ms.
        s.track_key = Some("other".into());
        assert_eq!(s.current_offset_ms(), 0.0);
        s.track_key = Some("k".into());
        assert_eq!(s.current_offset_ms(), 150.0);
    }

    // ── SyncReceived ──────────────────────────────────────────────────────────

    #[test]
    fn sync_received_updates_caption() {
        let mut s = test_state();
        s.cues = sample_cues();
        let _ = update(&mut s, Message::SyncReceived(paused_sync(6.5)));
        assert_eq!(s.caption, "foo");
    }

    #[test]
    fn sync_received_silent_when_disabled() {
        let mut s = test_state();
        s.enabled = false;
        s.cues = sample_cues();
        let _ = update(&mut s, Message::SyncReceived(paused_sync(2.0)));
        assert_eq!(s.caption, "");
    }

    // ── TimelineTick ──────────────────────────────────────────────────────────

    #[test]
    fn timeline_tick_updates_caption() {
        let mut s = test_state();
        s.cues = sample_cues();
        s.timeline_sync = Some(paused_sync(3.5));
        let _ = update(&mut s, Message::TimelineTick);
        assert_eq!(s.caption, "world");
    }

    #[test]
    fn timeline_tick_silent_when_disabled() {
        let mut s = test_state();
        s.enabled = false;
        s.cues = sample_cues();
        s.timeline_sync = Some(paused_sync(3.5));
        let _ = update(&mut s, Message::TimelineTick);
        assert_eq!(s.caption, "");
    }
}
