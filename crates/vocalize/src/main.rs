use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{container, text};
use iced::{Element, Subscription, Task};
use iced_layershell::to_layer_message;

mod config;
mod tray;
use config::VocalizeConfig;

/// App name: Wayland namespace, single-instance lock, config dir.
const APP: &str = "vocalize";
/// Tall fixed panel (chips + readout), like karaoke owns its geometry.
const SURFACE_H: u32 = 160;

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    SetEnabled(bool),
}

struct State {
    enabled: bool,
}

impl Default for State {
    fn default() -> Self {
        let cfg: VocalizeConfig = overlay::load_config(APP);
        State {
            enabled: cfg.enabled,
        }
    }
}

impl State {
    fn persist(&self) {
        overlay::save(
            APP,
            &VocalizeConfig {
                enabled: self.enabled,
                ..Default::default()
            },
        );
    }
}

impl overlay::OverlayApp for State {
    type Message = Message;
    fn namespace() -> &'static str {
        APP
    }
    fn margin_changed(margin: (i32, i32, i32, i32)) -> Message {
        Message::MarginChange(margin)
    }
    fn surface_height() -> u32 {
        SURFACE_H
    }
    fn stacks() -> bool {
        false
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        if let Message::SetEnabled(on) = message {
            self.enabled = on;
            self.persist();
        }
        Task::none()
    }
    fn view(&self) -> Element<'_, Message> {
        let body = if self.enabled {
            text("vocalize")
        } else {
            text("")
        };
        container(body)
            .center_x(iced::Fill)
            .center_y(iced::Fill)
            .into()
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(event_stream)
    }
}

fn event_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    let cfg: VocalizeConfig = overlay::load_config(APP);
    ksni::TrayService::new(tray::VocalizeTray {
        tx: tx.clone(),
        enabled: cfg.enabled,
    })
    .spawn();
    Box::pin(rx)
}

fn main() -> iced_layershell::Result {
    overlay::run::<State>()
}
