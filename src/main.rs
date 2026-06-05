use futures::channel::mpsc;
use futures::stream::BoxStream;
use futures::StreamExt;
use iced::widget::{container, text};
use iced::{Color, Element, Subscription, Task};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::LayerShellSettings;
use iced_layershell::to_layer_message;
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;

const WS_PORT: u16 = 8765;

#[derive(Deserialize)]
struct Caption {
    text: String,
}

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    CaptionReceived(String),
}

struct State {
    caption: String,
}

impl Default for State {
    fn default() -> Self {
        State { caption: String::new() }
    }
}

fn main() -> iced_layershell::Result {
    iced_layershell::application(
        State::default,
        "lyrics-on-screen",
        update,
        view,
    )
    .subscription(|_state| Subscription::run(ws_server_stream))
    .style(|_state, _theme| iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: Color::WHITE,
    })
    .layer_settings(LayerShellSettings {
        anchor: Anchor::Bottom | Anchor::Left | Anchor::Right,
        layer: Layer::Top,
        exclusive_zone: 0,
        size: Some((0, 80)), // 0 width = full screen (anchored L+R); height must be explicit
        margin: (0, 0, 40, 0),
        keyboard_interactivity: KeyboardInteractivity::None,
        events_transparent: false,
        ..Default::default()
    })
    .run()
}

fn update(state: &mut State, msg: Message) -> Task<Message> {
    if let Message::CaptionReceived(text) = msg {
        state.caption = text;
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    container(
        container(
            text(&state.caption)
                .size(32)
                .color(Color::WHITE),
        )
        .style(move |_theme| {
            if state.caption.is_empty() {
                container::Style::default()
            } else {
                container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.6))),
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

// Spawns a dedicated OS thread with its own tokio runtime to run the WebSocket
// server. Messages are forwarded to iced via a futures::channel::mpsc which is
// executor-agnostic and works across thread/runtime boundaries.
fn ws_server_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();

    std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build tokio runtime for WebSocket server")
            .block_on(run_server(tx));
    });

    Box::pin(rx)
}

async fn run_server(tx: mpsc::UnboundedSender<Message>) {
    // Retry bind with backoff so a restart doesn't cause a subscription spin-loop.
    let listener = loop {
        match TcpListener::bind(("127.0.0.1", WS_PORT)).await {
            Ok(l) => break l,
            Err(e) => {
                eprintln!("[lyrics-on-screen] bind failed on port {WS_PORT}: {e} — retrying in 5s");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    };
    eprintln!("[lyrics-on-screen] WebSocket server listening on ws://127.0.0.1:{WS_PORT}");

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let tx = tx.clone();
                tokio::spawn(async move {
                    match accept_async(stream).await {
                        Ok(mut ws) => {
                            while let Some(result) = ws.next().await {
                                match result {
                                    Ok(msg) => {
                                        let Ok(raw) = msg.to_text() else { continue };
                                        let Ok(caption) = serde_json::from_str::<Caption>(raw)
                                        else {
                                            continue;
                                        };
                                        if !caption.text.is_empty() {
                                            let _ = tx.unbounded_send(
                                                Message::CaptionReceived(caption.text),
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("[lyrics-on-screen] ws error from {addr}: {e}");
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
