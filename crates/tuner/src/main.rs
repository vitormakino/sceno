use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{canvas, column, container, text};
use iced::{Color, Element, Subscription, Task};
use iced_layershell::to_layer_message;

mod audio;
mod config;
mod meter;
mod note;
mod smooth;
use note::Note;

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    PitchUpdate(Option<Note>),
}

struct State {
    note: Option<Note>,
    enabled: bool,
    style: meter::MeterStyle,
    strobe_phase: f32,
}

impl Default for State {
    fn default() -> Self {
        let cfg: config::TunerConfig = overlay::load_config("tuner");
        State {
            note: None,
            enabled: cfg.enabled,
            style: meter::MeterStyle::from_idx(cfg.meter_style_idx),
            strobe_phase: 0.0,
        }
    }
}

impl overlay::OverlayApp for State {
    type Message = Message;
    fn namespace() -> &'static str {
        "tuner"
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        if let Message::PitchUpdate(n) = message {
            self.note = n;
        }
        Task::none()
    }
    fn view(&self) -> Element<'_, Message> {
        let empty = || container(text("")).center_x(iced::Fill).center_y(iced::Fill);
        if !self.enabled {
            return empty().into();
        }
        let Some(n) = &self.note else {
            return empty().into();
        };
        let color = meter::cents_color(n.cents);
        let cents_label = if note::is_in_tune(n.cents) {
            "IN TUNE".to_string()
        } else {
            format!("{:+.0}¢", n.cents)
        };
        let gauge = canvas(meter::Meter {
            cents: n.cents,
            style: self.style,
            phase: self.strobe_phase,
            color,
        })
        .width(iced::Fill)
        .height(iced::Length::Fixed(28.0));

        let body = column![
            text(format!("{}{}", n.name, n.octave))
                .size(40.0)
                .color(color),
            gauge,
            text(cents_label).size(18.0).color(color),
        ]
        .align_x(iced::Center)
        .spacing(2);

        container(
            container(body).padding([6, 18]).style(|_theme| {
                container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.45))),
                    border: iced::Border {
                        radius: 12.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            }),
        )
        .center_x(iced::Fill)
        .center_y(iced::Fill)
        .into()
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(audio_stream)
    }
}

fn audio_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    std::thread::spawn(move || audio::run(tx));
    Box::pin(rx)
}

fn main() -> iced_layershell::Result {
    overlay::run::<State>()
}
