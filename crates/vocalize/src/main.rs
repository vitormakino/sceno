use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{column, container, row, text};
use iced::{Color, Element, Subscription, Task};
use iced_layershell::to_layer_message;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod config;
mod exercise;
mod tray;
use config::VocalizeConfig;
use exercise::{Matcher, Mode, Scale, ScaleKind};
use pitch::Note;

/// App name: Wayland namespace, single-instance lock, config dir.
const APP: &str = "vocalize";
/// Tall fixed panel (chips + readout), like karaoke owns its geometry.
const SURFACE_H: u32 = 160;
/// How long the "Acertou!" success flash shows before the next item.
const FLASH: Duration = Duration::from_millis(450);

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    SetEnabled(bool),
    /// Smoothed fundamental frequency (Hz) from the mic, or `None` on silence.
    PitchUpdate(Option<f64>),
    /// 33 ms UI tick driving listen/sustain timing and the success flash.
    Tick,
}

struct State {
    enabled: bool,
    scale: Scale,
    mode: Mode,
    cents_window: f64,
    sustain_ms: f64,
    /// Target MIDI notes of the current item (playback octave).
    item: Vec<i64>,
    matcher: Matcher,
    prev_degree: usize,
    rng: u64,
    /// Continuous MIDI value of the current sung pitch (for the matcher).
    sung: Option<f64>,
    /// Current sung note (for the readout).
    sung_note: Option<Note>,
    /// Timestamp of the previous `Tick`, for the frame delta.
    last_tick: Instant,
    /// While `Some` and not yet elapsed, the success flash is showing.
    success_until: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        let cfg: VocalizeConfig = overlay::load_config(APP);
        let scale = Scale {
            root: cfg.scale_root,
            kind: ScaleKind::from_idx(cfg.scale_kind_idx),
        };
        let mode = Mode::from_idx(cfg.mode_idx);
        let cents_window = cfg.cents_window;
        let sustain_ms = cfg.sustain_ms as f64;
        let mut rng = seed();
        let degree = next_degree(&mut rng, scale.degree_count(), usize::MAX);
        let item = exercise::item_at(&scale, mode, degree);
        let matcher = Matcher::new(&item, cents_window, sustain_ms);
        State {
            enabled: cfg.enabled,
            scale,
            mode,
            cents_window,
            sustain_ms,
            item,
            matcher,
            prev_degree: degree,
            rng,
            sung: None,
            sung_note: None,
            last_tick: Instant::now(),
            success_until: None,
        }
    }
}

/// Seed the RNG from the wall clock (odd, non-zero).
fn seed() -> u64 {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    n | 1
}

/// xorshift64 step.
fn xorshift(s: &mut u64) -> u64 {
    let mut x = *s;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *s = x;
    x
}

/// Pick the next scale-degree index, avoiding an immediate repeat when possible.
fn next_degree(s: &mut u64, count: usize, prev: usize) -> usize {
    if count <= 1 {
        return 0;
    }
    let mut d = (xorshift(s) % count as u64) as usize;
    if d == prev {
        d = (d + 1) % count;
    }
    d
}

impl State {
    /// Move to a fresh random item and rebuild the matcher.
    fn advance(&mut self) {
        let degree = next_degree(&mut self.rng, self.scale.degree_count(), self.prev_degree);
        self.prev_degree = degree;
        self.item = exercise::item_at(&self.scale, self.mode, degree);
        self.matcher = Matcher::new(&self.item, self.cents_window, self.sustain_ms);
    }

    fn persist(&self) {
        overlay::save(
            APP,
            &VocalizeConfig {
                enabled: self.enabled,
                audible: true,
                scale_root: self.scale.root,
                scale_kind_idx: self.scale.kind.index(),
                mode_idx: self.mode.index(),
                cents_window: self.cents_window,
                sustain_ms: self.sustain_ms as u64,
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
        match message {
            Message::SetEnabled(on) => {
                self.enabled = on;
                self.persist();
            }
            Message::PitchUpdate(freq) => {
                self.sung_note = freq.map(|f| pitch::frequency_to_note(f, pitch::A4));
                self.sung = self.sung_note.map(|n| n.midi + n.cents / 100.0);
            }
            Message::Tick => {
                let now = Instant::now();
                let dt = (now - self.last_tick).as_secs_f64() * 1000.0;
                self.last_tick = now;
                if let Some(t) = self.success_until {
                    if now >= t {
                        self.success_until = None;
                        self.advance();
                    }
                    return Task::none();
                }
                let newly = self.matcher.update(self.sung, dt);
                if !newly.is_empty() && self.matcher.all_collected() {
                    self.success_until = Some(now + FLASH);
                }
            }
            _ => {}
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
        let collected = self.matcher.collected();
        let flashing = self.success_until.is_some();
        let mut chips = row![].spacing(12);
        for (i, &m) in self.item.iter().enumerate() {
            let done = collected.get(i).copied().unwrap_or(false);
            let color = if done {
                Color::from_rgb(0.30, 0.90, 0.30)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.45)
            };
            chips = chips.push(text(exercise::note_label(m)).size(34.0).color(color));
        }
        let prompt = if flashing { "Acertou!" } else { "Cante:" };
        let (you_label, you_color) = match self.sung_note {
            Some(n) => (
                format!("Você: {}{} {:+.0}¢", n.name, n.octave, n.cents),
                Color::from_rgba(0.85, 0.85, 0.85, 0.9),
            ),
            None => (
                "Você: — (microfone?)".to_string(),
                Color::from_rgba(1.0, 1.0, 1.0, 0.6),
            ),
        };
        let body = column![
            text(prompt).size(18.0).color(Color::WHITE),
            chips,
            text(you_label).size(16.0).color(you_color),
        ]
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
        let events = Subscription::run(event_stream);
        if self.enabled {
            Subscription::batch([events, Subscription::run(tick_stream)])
        } else {
            events
        }
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
    std::thread::spawn(move || {
        pitch::run_capture(|freq| tx.unbounded_send(Message::PitchUpdate(freq)).is_ok());
    });
    Box::pin(rx)
}

fn tick_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(33));
            if tx.unbounded_send(Message::Tick).is_err() {
                break;
            }
        }
    });
    Box::pin(rx)
}

fn main() -> iced_layershell::Result {
    overlay::run::<State>()
}
