use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{column, container, progress_bar, text};
use iced::{Color, Element, Subscription, Task};
use iced_layershell::to_layer_message;

mod audio;
mod note;
use note::Note;

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    PitchUpdate(Option<Note>),
}

#[derive(Default)]
struct State {
    note: Option<Note>,
}

impl overlay::OverlayApp for State {
    type Message = Message;
    fn namespace() -> &'static str { "tuner" }
    fn update(&mut self, message: Message) -> Task<Message> {
        if let Message::PitchUpdate(n) = message {
            self.note = n;
        }
        Task::none()
    }
    fn view(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = match &self.note {
            None => text("").into(),
            Some(n) => {
                let in_tune = note::is_in_tune(n.cents);
                let pos = note::meter_position(n.cents) as f32;
                let color = if in_tune { Color::from_rgb(0.3, 0.9, 0.3) } else { Color::WHITE };
                column![
                    text(format!("{}{}", n.name, n.octave)).size(44.0).color(color),
                    progress_bar(0.0..=1.0, pos).girth(8.0),
                    text(format!("{:+.0}¢", n.cents)).size(20.0).color(color),
                ]
                .align_x(iced::Center)
                .spacing(4)
                .into()
            }
        };
        container(content).center_x(iced::Fill).center_y(iced::Fill).into()
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
