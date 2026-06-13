use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{container, text};
use iced::{Color, Element, Subscription, Task};
use iced_layershell::to_layer_message;

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
        let label = match &self.note {
            Some(n) => format!("{}{}", n.name, n.octave),
            None => String::new(),
        };
        container(text(label).size(44.0).color(Color::WHITE))
            .center_x(iced::Fill)
            .center_y(iced::Fill)
            .into()
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }
}

fn main() -> iced_layershell::Result {
    overlay::run::<State>()
}

// Placeholder to keep BoxStream/mpsc imports used until the audio task wires them.
#[allow(dead_code)]
fn _unused() -> BoxStream<'static, Message> {
    let (_tx, rx) = mpsc::unbounded::<Message>();
    Box::pin(rx)
}
