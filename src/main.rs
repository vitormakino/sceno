use futures::channel::mpsc;
use futures::stream::BoxStream;
use futures::StreamExt;
use iced::widget::{container, text};
use iced::{Color, Element, Subscription, Task};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::LayerShellSettings;
use iced_layershell::to_layer_message;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::accept_async;

const WS_PORT: u16 = 8765;
const CLEAR_AFTER_SECS: u64 = 5;

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
}

// ── App types ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Caption {
    text: String,
}

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    CaptionReceived(String),
    ClearCaption,
    SetEnabled(bool),
    SetFontSize(FontSize),
}

struct State {
    caption: String,
    enabled: bool,
    font_size: FontSize,
}

impl Default for State {
    fn default() -> Self {
        State {
            caption: String::new(),
            enabled: true,
            font_size: FontSize::Medium,
        }
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
                submenu: vec![
                    RadioGroup {
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
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
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
                    .into(),
                ],
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

fn main() -> iced_layershell::Result {
    iced_layershell::application(State::default, "lyrics-on-screen", update, view)
        .subscription(|_state| Subscription::run(ws_server_stream))
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
            if state.enabled {
                state.caption = t;
            }
        }
        Message::ClearCaption => {
            state.caption.clear();
        }
        Message::SetEnabled(e) => {
            state.enabled = e;
            if !e {
                state.caption.clear();
            }
        }
        Message::SetFontSize(s) => {
            state.font_size = s;
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

// ── WebSocket server + tray bootstrap ─────────────────────────────────────────

fn ws_server_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();

    // System tray — ksni manages its own D-Bus thread internally
    ksni::TrayService::new(LyricsTray {
        tx: tx.clone(),
        enabled: true,
        font_size: FontSize::Medium,
        position: Position::Bottom,
    })
    .spawn();

    // WebSocket server — needs its own tokio runtime (iced_layershell is not tokio)
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
            .block_on(run_server(tx));
    });

    Box::pin(rx)
}

async fn run_server(tx: mpsc::UnboundedSender<Message>) {
    let listener = loop {
        match TcpListener::bind(("127.0.0.1", WS_PORT)).await {
            Ok(l) => break l,
            Err(e) => {
                eprintln!("[lyrics-on-screen] bind failed: {e} — retrying in 5s");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    };
    eprintln!("[lyrics-on-screen] WebSocket server listening on ws://127.0.0.1:{WS_PORT}");

    let clear_handle: Arc<Mutex<Option<JoinHandle<()>>>> = Arc::new(Mutex::new(None));

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let tx = tx.clone();
                let clear_handle = clear_handle.clone();
                tokio::spawn(async move {
                    match accept_async(stream).await {
                        Ok(mut ws) => {
                            while let Some(result) = ws.next().await {
                                match result {
                                    Ok(msg) => {
                                        let Ok(raw) = msg.to_text() else { continue };
                                        let Ok(caption) =
                                            serde_json::from_str::<Caption>(raw)
                                        else {
                                            continue;
                                        };
                                        if caption.text.is_empty() {
                                            continue;
                                        }
                                        let _ = tx
                                            .unbounded_send(Message::CaptionReceived(caption.text));

                                        let mut guard = clear_handle.lock().unwrap();
                                        if let Some(h) = guard.take() {
                                            h.abort();
                                        }
                                        let tx2 = tx.clone();
                                        *guard = Some(tokio::spawn(async move {
                                            tokio::time::sleep(std::time::Duration::from_secs(
                                                CLEAR_AFTER_SECS,
                                            ))
                                            .await;
                                            let _ = tx2.unbounded_send(Message::ClearCaption);
                                        }));
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "[lyrics-on-screen] ws error from {addr}: {e}"
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[lyrics-on-screen] handshake failed from {addr}: {e}");
                        }
                    }
                });
            }
            Err(e) => {
                eprintln!("[lyrics-on-screen] accept error: {e} — retrying in 1s");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}
