use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{container, text};
use iced::{Color, Element, Subscription, Task};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::LayerShellSettings;
use iced_layershell::to_layer_message;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::os::unix::io::AsRawFd;
use std::time::Instant;

const CLEAR_AFTER_SECS: u64 = 5;
const LOCK_PATH: &str = "/tmp/lyrics-on-screen.lock";

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

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Live,
    Timeline,
}

impl Mode {
    fn index(self) -> usize {
        match self {
            Mode::Live => 0,
            Mode::Timeline => 1,
        }
    }
    fn from_idx(i: usize) -> Self {
        if i == 1 { Mode::Timeline } else { Mode::Live }
    }
}

// ── Persistent config ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct SavedConfig {
    #[serde(default = "default_font_idx")]
    font_size_idx: usize,
    #[serde(default)]
    mode_idx: usize,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_font_idx() -> usize { 1 }
fn default_enabled() -> bool { true }

impl Default for SavedConfig {
    fn default() -> Self {
        SavedConfig { font_size_idx: 1, mode_idx: 0, enabled: true }
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
    let Some(path) = config_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cfg = SavedConfig {
        font_size_idx: state.font_size.index(),
        mode_idx: state.mode.index(),
        enabled: state.enabled,
    };
    if let Ok(json) = serde_json::to_string(&cfg) {
        let _ = std::fs::write(path, json);
    }
}

// ── Timeline types ────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug, Clone)]
struct CueEntry {
    start: f64,
    end: f64,
    text: String,
}

/// Reference point that lets us extrapolate the current video position
/// between sync messages from the extension.
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

// ── Native message types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(untagged)]
enum NativeMsg {
    // Live mode: { "text": "...", "source": "..." }
    Text { text: String },
    // Timeline cue window: { "type": "cues", "cues": [...], "currentTime": ..., ... }
    // Must come before Sync so the required `cues` field is checked first.
    Cues {
        cues: Vec<CueEntry>,
        #[serde(rename = "currentTime")]
        current_time: f64,
        #[serde(default)]
        paused: bool,
        #[serde(rename = "playbackRate", default = "default_rate")]
        playback_rate: f64,
    },
    // Sync heartbeat: { "type": "sync", "currentTime": ..., ... }
    Sync {
        #[serde(rename = "currentTime")]
        current_time: f64,
        #[serde(default)]
        paused: bool,
        #[serde(rename = "playbackRate", default = "default_rate")]
        playback_rate: f64,
    },
}

fn default_rate() -> f64 { 1.0 }

// ── App types ─────────────────────────────────────────────────────────────────

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    CaptionReceived(String),
    ClearCaption,
    SetEnabled(bool),
    SetFontSize(FontSize),
    SetMode(Mode),
    CuesReceived(Vec<CueEntry>, TimelineSync),
    SyncReceived(TimelineSync),
    TimelineTick,
}

struct State {
    caption: String,
    last_live_caption: String,
    enabled: bool,
    font_size: FontSize,
    mode: Mode,
    cues: Vec<CueEntry>,
    timeline_sync: Option<TimelineSync>,
    paused: bool,
}

impl Default for State {
    fn default() -> Self {
        let cfg = load_config();
        State {
            caption: String::new(),
            last_live_caption: String::new(),
            enabled: cfg.enabled,
            font_size: FontSize::from_idx(cfg.font_size_idx),
            mode: Mode::from_idx(cfg.mode_idx),
            cues: Vec::new(),
            timeline_sync: None,
            paused: false,
        }
    }
}

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
    mode: Mode,
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
        let mode = self.mode;
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
                label: "Modo".into(),
                submenu: vec![RadioGroup {
                    selected: mode.index(),
                    select: Box::new(|this: &mut Self, idx| {
                        this.mode = if idx == 0 { Mode::Live } else { Mode::Timeline };
                        let _ = this.tx.unbounded_send(Message::SetMode(this.mode));
                    }),
                    options: vec![
                        RadioItem { label: "Live".into(), ..Default::default() },
                        RadioItem { label: "Timeline".into(), ..Default::default() },
                    ],
                }
                .into()],
                ..Default::default()
            }
            .into(),
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

// ── iced app ──────────────────────────────────────────────────────────────────

fn ensure_single_instance() {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(LOCK_PATH)
        .unwrap_or_else(|e| {
            eprintln!("[lyrics-on-screen] não foi possível abrir lock file: {e}");
            std::process::exit(1);
        });
    // LOCK_EX | LOCK_NB — exclusive, non-blocking
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret != 0 {
        eprintln!("[lyrics-on-screen] já está em execução");
        std::process::exit(0);
    }
    // Keep fd open for the process lifetime; kernel releases the lock on exit.
    std::mem::forget(file);
}

fn main() -> iced_layershell::Result {
    ensure_single_instance();
    iced_layershell::application(State::default, "lyrics-on-screen", update, view)
        .subscription(|state| {
            let native = Subscription::run(native_msg_stream);
            // Only tick when playing — paused video doesn't need interpolation.
            let needs_tick = state.mode == Mode::Timeline
                && !state.cues.is_empty()
                && !state.paused;
            if needs_tick {
                Subscription::batch([native, Subscription::run(timeline_tick_stream)])
            } else {
                native
            }
        })
        .style(|_state, _theme| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: Color::WHITE,
        })
        .layer_settings(LayerShellSettings {
            anchor: Anchor::Bottom | Anchor::Left | Anchor::Right,
            layer: Layer::Top,
            exclusive_zone: 0,
            size: Some((0, 80)),
            margin: (0, 0, 40, 0),
            keyboard_interactivity: KeyboardInteractivity::None,
            events_transparent: true,
            ..Default::default()
        })
        .run()
}

fn update(state: &mut State, msg: Message) -> Task<Message> {
    match msg {
        Message::CaptionReceived(t) => {
            state.last_live_caption = t.clone();
            if state.enabled && state.mode == Mode::Live {
                state.caption = t;
            }
        }
        Message::ClearCaption => {
            // Only the Live-mode auto-clear timer fires this; ignore in Timeline.
            if state.mode == Mode::Live {
                state.caption.clear();
                state.last_live_caption.clear();
            }
        }
        Message::SetEnabled(e) => {
            state.enabled = e;
            if !e {
                state.caption.clear();
            } else if state.mode == Mode::Live {
                // Restore the last known caption immediately; avoids a blank
                // screen until the next cuechange fires.
                state.caption = state.last_live_caption.clone();
            } else {
                // Timeline: apply immediately from current sync position.
                apply_timeline_caption(state);
            }
            save_config(state);
        }
        Message::SetFontSize(s) => {
            state.font_size = s;
            save_config(state);
        }
        Message::SetMode(m) => {
            state.mode = m;
            state.caption.clear();
            if m == Mode::Live {
                state.cues.clear();
                state.timeline_sync = None;
                state.paused = false;
            }
            save_config(state);
        }
        Message::CuesReceived(cues, sync) => {
            state.paused = sync.paused;
            state.cues = cues;
            state.timeline_sync = Some(sync);
            if state.mode == Mode::Timeline && state.enabled {
                apply_timeline_caption(state);
            }
        }
        Message::SyncReceived(sync) => {
            state.paused = sync.paused;
            state.timeline_sync = Some(sync);
            if state.mode == Mode::Timeline && state.enabled {
                apply_timeline_caption(state);
            }
        }
        Message::TimelineTick => {
            if state.mode == Mode::Timeline && state.enabled {
                if let Some(sync) = &state.timeline_sync {
                    state.caption =
                        cue_at(&state.cues, sync.current_time()).map(String::from).unwrap_or_default();
                }
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

fn native_msg_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    let cfg = load_config();

    ksni::TrayService::new(LyricsTray {
        tx: tx.clone(),
        enabled: cfg.enabled,
        font_size: FontSize::from_idx(cfg.font_size_idx),
        position: Position::Bottom,
        mode: Mode::from_idx(cfg.mode_idx),
    })
    .spawn();

    std::thread::spawn(move || read_native_messages(tx));

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

fn read_native_messages(tx: mpsc::UnboundedSender<Message>) {
    // Use Instant for monotonic auto-clear timing; avoids sensitivity to clock changes.
    let last_activity: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));

    {
        let last_activity = last_activity.clone();
        let tx = tx.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let mut guard = last_activity.lock().unwrap();
            if let Some(t) = *guard {
                if t.elapsed().as_secs() >= CLEAR_AFTER_SECS {
                    *guard = None;
                    drop(guard);
                    let _ = tx.unbounded_send(Message::ClearCaption);
                }
            }
        });
    }

    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();

    loop {
        let mut len_buf = [0u8; 4];
        if stdin.read_exact(&mut len_buf).is_err() {
            break;
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        if stdin.read_exact(&mut buf).is_err() {
            break;
        }

        let Ok(msg) = serde_json::from_slice::<NativeMsg>(&buf) else {
            continue;
        };

        match msg {
            NativeMsg::Text { text } if !text.is_empty() => {
                *last_activity.lock().unwrap() = Some(Instant::now());
                let _ = tx.unbounded_send(Message::CaptionReceived(text));
            }
            NativeMsg::Cues { cues, current_time, paused, playback_rate } => {
                let sync = TimelineSync {
                    video_time: current_time,
                    captured_at: Instant::now(),
                    paused,
                    playback_rate,
                };
                let _ = tx.unbounded_send(Message::CuesReceived(cues, sync));
            }
            NativeMsg::Sync { current_time, paused, playback_rate } => {
                let sync = TimelineSync {
                    video_time: current_time,
                    captured_at: Instant::now(),
                    paused,
                    playback_rate,
                };
                let _ = tx.unbounded_send(Message::SyncReceived(sync));
            }
            _ => {}
        }
    }

    eprintln!("[lyrics-on-screen] stdin fechado — encerrando");
    std::process::exit(0);
}
