use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{container, text};
use iced::{Color, Element, Subscription, Task};
use iced_layershell::reexport::Anchor;
use iced_layershell::to_layer_message;
use serde::{Deserialize, Serialize};
use std::time::Instant;

mod lrc;
mod lrclib;
mod player;

// ── Settings types ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Position {
    Bottom,
    Top,
}

impl Position {
    fn anchor(self) -> Anchor {
        match self {
            Position::Bottom => Anchor::Bottom | Anchor::Left | Anchor::Right,
            Position::Top => Anchor::Top | Anchor::Left | Anchor::Right,
        }
    }
    fn margin(self) -> (i32, i32, i32, i32) {
        match self {
            Position::Bottom => (0, 0, 40, 0),
            Position::Top => (40, 0, 0, 0),
        }
    }
    fn index(self) -> usize {
        match self {
            Position::Bottom => 0,
            Position::Top => 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum FontSize {
    Small,
    Medium,
    Large,
}

impl FontSize {
    fn px(self) -> f32 {
        match self {
            FontSize::Small => 22.0,
            FontSize::Medium => 32.0,
            FontSize::Large => 44.0,
        }
    }
    fn index(self) -> usize {
        match self {
            FontSize::Small => 0,
            FontSize::Medium => 1,
            FontSize::Large => 2,
        }
    }
    fn from_idx(i: usize) -> Self {
        match i {
            0 => FontSize::Small,
            2 => FontSize::Large,
            _ => FontSize::Medium,
        }
    }
}

// ── Persistent config ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct SavedConfig {
    #[serde(default = "default_font_idx")]
    font_size_idx: usize,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_font_idx() -> usize { 1 }
fn default_enabled() -> bool { true }

impl Default for SavedConfig {
    fn default() -> Self {
        SavedConfig { font_size_idx: 1, enabled: true }
    }
}

fn config_path() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(|home| {
        std::path::PathBuf::from(home).join(".config/lyrics-on-screen/config.json")
    })
}

fn load_config() -> SavedConfig {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(state: &State) {
    if cfg!(test) { return; }
    let Some(path) = config_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cfg = SavedConfig {
        font_size_idx: state.font_size.index(),
        enabled: state.enabled,
    };
    if let Ok(json) = serde_json::to_string(&cfg) {
        let _ = std::fs::write(path, json);
    }
}

// ── Timeline types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct CueEntry {
    start: f64,
    end: f64,
    text: String,
}

/// Reference point that lets us extrapolate the current playback position
/// between sync samples from the player.
#[derive(Debug, Clone)]
struct TimelineSync {
    video_time: f64,
    captured_at: Instant,
    paused: bool,
    playback_rate: f64,
}

impl TimelineSync {
    fn current_time(&self) -> f64 {
        if self.paused {
            self.video_time
        } else {
            self.video_time + self.captured_at.elapsed().as_secs_f64() * self.playback_rate
        }
    }
}

fn cue_at(cues: &[CueEntry], t: f64) -> Option<&str> {
    cues.iter()
        .find(|c| c.start <= t && t < c.end)
        .map(|c| c.text.as_str())
}

// ── App types ─────────────────────────────────────────────────────────────────

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    SetEnabled(bool),
    SetFontSize(FontSize),
    CuesReceived(Vec<CueEntry>, TimelineSync),
    SyncReceived(TimelineSync),
    TimelineTick,
}

struct State {
    caption: String,
    enabled: bool,
    font_size: FontSize,
    cues: Vec<CueEntry>,
    timeline_sync: Option<TimelineSync>,
    paused: bool,
}

impl Default for State {
    fn default() -> Self {
        let cfg = load_config();
        State {
            caption: String::new(),
            enabled: cfg.enabled,
            font_size: FontSize::from_idx(cfg.font_size_idx),
            cues: Vec::new(),
            timeline_sync: None,
            paused: false,
        }
    }
}

/// Recompute the visible caption from the current cues and sync position.
fn apply_timeline_caption(state: &mut State) {
    if let Some(sync) = &state.timeline_sync {
        state.caption = cue_at(&state.cues, sync.current_time())
            .map(String::from)
            .unwrap_or_default();
    }
}

// ── System tray ───────────────────────────────────────────────────────────────

struct LyricsTray {
    tx: mpsc::UnboundedSender<Message>,
    enabled: bool,
    font_size: FontSize,
    position: Position,
}

impl ksni::Tray for LyricsTray {
    fn icon_name(&self) -> String {
        "audio-x-generic".into()
    }
    fn title(&self) -> String {
        "Lyrics on Screen".into()
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let pos = self.position;
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
            MenuItem::Separator,
            SubMenu {
                label: "Posição".into(),
                submenu: vec![RadioGroup {
                    selected: pos.index(),
                    select: Box::new(|this: &mut Self, idx| {
                        this.position = match idx {
                            0 => Position::Bottom,
                            _ => Position::Top,
                        };
                        let _ = this
                            .tx
                            .unbounded_send(Message::AnchorChange(this.position.anchor()));
                        let _ = this
                            .tx
                            .unbounded_send(Message::MarginChange(this.position.margin()));
                    }),
                    options: vec![
                        RadioItem { label: "Baixo".into(), ..Default::default() },
                        RadioItem { label: "Topo".into(), ..Default::default() },
                    ],
                }
                .into()],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Tamanho da fonte".into(),
                submenu: vec![RadioGroup {
                    selected: fs.index(),
                    select: Box::new(|this: &mut Self, idx| {
                        this.font_size = match idx {
                            0 => FontSize::Small,
                            1 => FontSize::Medium,
                            _ => FontSize::Large,
                        };
                        let _ = this
                            .tx
                            .unbounded_send(Message::SetFontSize(this.font_size));
                    }),
                    options: vec![
                        RadioItem { label: "Pequeno".into(), ..Default::default() },
                        RadioItem { label: "Médio".into(), ..Default::default() },
                        RadioItem { label: "Grande".into(), ..Default::default() },
                    ],
                }
                .into()],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
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
        "lyrics-on-screen"
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
            save_config(state);
        }
        Message::SetFontSize(s) => {
            state.font_size = s;
            save_config(state);
        }
        Message::CuesReceived(cues, sync) => {
            state.paused = sync.paused;
            state.cues = cues;
            state.timeline_sync = Some(sync);
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
        Message::TimelineTick => {
            if state.enabled {
                apply_timeline_caption(state);
            }
        }
        _ => {}
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    container(
        container(
            text(&state.caption)
                .size(state.font_size.px())
                .color(Color::WHITE),
        )
        .style(move |_theme| {
            if state.caption.is_empty() {
                container::Style::default()
            } else {
                container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba(
                        0.0, 0.0, 0.0, 0.6,
                    ))),
                    border: iced::Border { radius: 6.0.into(), ..Default::default() },
                    ..Default::default()
                }
            }
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
    let cfg = load_config();

    ksni::TrayService::new(LyricsTray {
        tx: tx.clone(),
        enabled: cfg.enabled,
        font_size: FontSize::from_idx(cfg.font_size_idx),
        position: Position::Bottom,
    })
    .spawn();

    std::thread::spawn(move || player::run(tx));

    Box::pin(rx)
}

fn timeline_tick_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if tx.unbounded_send(Message::TimelineTick).is_err() {
            break;
        }
    });
    Box::pin(rx)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn test_state() -> State {
        State {
            caption: String::new(),
            enabled: true,
            font_size: FontSize::Medium,
            cues: Vec::new(),
            timeline_sync: None,
            paused: false,
        }
    }

    fn paused_sync(t: f64) -> TimelineSync {
        TimelineSync { video_time: t, captured_at: Instant::now(), paused: true, playback_rate: 1.0 }
    }

    fn sample_cues() -> Vec<CueEntry> {
        vec![
            CueEntry { start: 1.0, end: 3.0, text: "hello".into() },
            CueEntry { start: 3.0, end: 5.0, text: "world".into() },
            CueEntry { start: 5.0, end: 7.0, text: "foo".into() },
        ]
    }

    // ── cue_at ────────────────────────────────────────────────────────────────

    #[test]
    fn cue_at_returns_active_cue() {
        let cues = sample_cues();
        assert_eq!(cue_at(&cues, 1.0), Some("hello"));
        assert_eq!(cue_at(&cues, 2.9), Some("hello"));
        assert_eq!(cue_at(&cues, 3.0), Some("world")); // start é inclusivo
        assert_eq!(cue_at(&cues, 4.5), Some("world"));
        assert_eq!(cue_at(&cues, 6.0), Some("foo"));
    }

    #[test]
    fn cue_at_none_outside_cues() {
        let cues = vec![
            CueEntry { start: 1.0, end: 2.0, text: "a".into() },
            CueEntry { start: 3.0, end: 4.0, text: "b".into() },
        ];
        assert_eq!(cue_at(&cues, 0.5), None);
        assert_eq!(cue_at(&cues, 2.0), None); // end é exclusivo
        assert_eq!(cue_at(&cues, 2.5), None); // gap entre cues
        assert_eq!(cue_at(&cues, 4.0), None);
    }

    #[test]
    fn cue_at_empty_list() {
        assert_eq!(cue_at(&[], 1.0), None);
    }

    // ── TimelineSync::current_time ────────────────────────────────────────────

    #[test]
    fn current_time_fixed_when_paused() {
        let sync = paused_sync(42.5);
        assert_eq!(sync.current_time(), 42.5);
    }

    #[test]
    fn current_time_advances_when_playing() {
        let sync = TimelineSync {
            video_time: 10.0,
            captured_at: Instant::now() - Duration::from_secs(2),
            paused: false,
            playback_rate: 1.0,
        };
        let t = sync.current_time();
        assert!((12.0..12.1).contains(&t), "expected ~12.0, got {t}");
    }

    #[test]
    fn current_time_respects_playback_rate() {
        let sync = TimelineSync {
            video_time: 0.0,
            captured_at: Instant::now() - Duration::from_secs(2),
            paused: false,
            playback_rate: 2.0,
        };
        let t = sync.current_time();
        assert!((4.0..4.1).contains(&t), "2× speed: expected ~4.0, got {t}");
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
    fn cues_received_updates_caption_and_paused() {
        let mut s = test_state();
        let sync = paused_sync(2.0);
        let _ = update(&mut s, Message::CuesReceived(sample_cues(), sync));
        assert_eq!(s.caption, "hello");
        assert!(s.paused);
    }

    #[test]
    fn cues_received_silent_when_disabled() {
        let mut s = test_state();
        s.enabled = false;
        let _ = update(&mut s, Message::CuesReceived(sample_cues(), paused_sync(2.0)));
        assert_eq!(s.caption, "");
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

    // ── FontSize helpers ──────────────────────────────────────────────────────

    #[test]
    fn fontsize_from_idx_roundtrips() {
        for (i, expected) in [(0, FontSize::Small), (1, FontSize::Medium), (2, FontSize::Large)] {
            assert_eq!(FontSize::from_idx(i), expected);
            assert_eq!(expected.index(), i);
        }
    }

    #[test]
    fn fontsize_unknown_idx_defaults_to_medium() {
        assert_eq!(FontSize::from_idx(99), FontSize::Medium);
    }

    // ── SavedConfig serialization ─────────────────────────────────────────────

    #[test]
    fn saved_config_roundtrips_json() {
        let cfg = SavedConfig { font_size_idx: 2, enabled: false };
        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: SavedConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.font_size_idx, 2);
        assert!(!loaded.enabled);
    }

    #[test]
    fn saved_config_missing_fields_use_defaults() {
        let cfg: SavedConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.font_size_idx, 1);  // Medium
        assert!(cfg.enabled);
    }

    #[test]
    fn saved_config_ignores_legacy_mode_idx() {
        // Old configs carried a mode_idx field; it must be ignored, not rejected.
        let cfg: SavedConfig =
            serde_json::from_str(r#"{"font_size_idx":2,"mode_idx":1,"enabled":true}"#).unwrap();
        assert_eq!(cfg.font_size_idx, 2);
        assert!(cfg.enabled);
    }
}
