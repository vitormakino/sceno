use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{column, container, row, text};
use iced::{Color, Element, Subscription, Task};
use iced_layershell::to_layer_message;

mod config;
mod exercise;
mod tray;
use config::VocalizeConfig;
use exercise::{Mode, Scale, ScaleKind};

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
    scale: Scale,
    mode: Mode,
    /// Target MIDI notes of the current item (playback octave).
    item: Vec<i64>,
}

impl Default for State {
    fn default() -> Self {
        let cfg: VocalizeConfig = overlay::load_config(APP);
        let scale = Scale {
            root: cfg.scale_root,
            kind: ScaleKind::from_idx(cfg.scale_kind_idx),
        };
        let mode = Mode::from_idx(cfg.mode_idx);
        let item = exercise::item_at(&scale, mode, 0);
        State {
            enabled: cfg.enabled,
            scale,
            mode,
            item,
        }
    }
}

impl State {
    fn persist(&self) {
        overlay::save(
            APP,
            &VocalizeConfig {
                enabled: self.enabled,
                audible: true,
                scale_root: self.scale.root,
                scale_kind_idx: self.scale.kind.index(),
                mode_idx: self.mode.index(),
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
        let empty = || {
            container(text(""))
                .center_x(iced::Fill)
                .center_y(iced::Fill)
        };
        if !self.enabled {
            return empty().into();
        }
        let mut chips = row![].spacing(12);
        for &m in &self.item {
            chips = chips.push(
                text(exercise::note_label(m))
                    .size(34.0)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.85)),
            );
        }
        let body = column![text("Cante:").size(18.0).color(Color::WHITE), chips]
            .align_x(iced::Center)
            .spacing(8);
        container(
            container(body)
                .padding([10, 18])
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
