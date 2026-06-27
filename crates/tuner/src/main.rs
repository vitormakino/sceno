// The macOS build compiles without the tray; the menu/option helpers it would
// use are then unused. Silence dead-code there rather than cfg-gate each one.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{canvas, column, container, text};
use iced::{Color, Element, Subscription, Task};

mod audio;
mod config;
mod instrument;
mod meter;
#[cfg(target_os = "linux")]
mod tray;
use instrument::Instrument;
use pitch::Note;

/// Selectable reference pitches (Hz) for A4, offered in the tray.
pub const REFERENCES: [f64; 4] = [432.0, 440.0, 442.0, 443.0];

#[cfg_attr(target_os = "linux", iced_layershell::to_layer_message)]
#[derive(Debug, Clone)]
enum Message {
    /// Smoothed fundamental frequency (Hz) from the mic, or `None` on silence.
    PitchUpdate(Option<f64>),
    SetEnabled(bool),
    SetMeterStyle(meter::MeterStyle),
    /// Change the A4 reference pitch (Hz).
    SetReference(f64),
    /// Change the instrument tuning preset.
    SetInstrument(Instrument),
    StrobeTick,
    /// The on-disk config changed (external edit); reload if it actually differs.
    ReloadConfig,
    /// Rewrite the config with defaults and rebuild from it (tray "Restaurar padrões").
    ResetDefaults,
}

struct State {
    /// Last smoothed frequency (Hz), kept so a reference/instrument change re-maps
    /// the readout without waiting for the next mic frame.
    last_freq: Option<f64>,
    note: Option<Note>,
    enabled: bool,
    style: meter::MeterStyle,
    a4_hz: f64,
    instrument: Instrument,
    strobe_phase: f32,
}

impl Default for State {
    fn default() -> Self {
        let cfg: config::TunerConfig = overlay::load_or_seed("tuner");
        State {
            last_freq: None,
            note: None,
            enabled: cfg.enabled,
            style: meter::MeterStyle::from_idx(cfg.meter_style_idx),
            a4_hz: cfg.a4_hz,
            instrument: Instrument::from_idx(cfg.instrument_idx),
            strobe_phase: 0.0,
        }
    }
}

/// Map a frequency to the displayed note: nearest chromatic note (chromatic
/// preset) or the nearest open string (instrument preset).
fn note_from(freq: f64, a4: f64, inst: Instrument) -> Note {
    match pitch::nearest_target(freq, a4, inst.targets()) {
        Some((midi, cents)) => Note::at_midi(midi, cents),
        None => pitch::frequency_to_note(freq, a4),
    }
}

impl State {
    /// Recompute the displayed note from the last frequency under the current
    /// reference and instrument.
    fn remap(&mut self) {
        self.note = self
            .last_freq
            .map(|f| note_from(f, self.a4_hz, self.instrument));
    }

    /// The current settings as a serializable config (for persisting / comparing).
    fn current_config(&self) -> config::TunerConfig {
        config::TunerConfig {
            meter_style_idx: self.style.index(),
            enabled: self.enabled,
            a4_hz: self.a4_hz,
            instrument_idx: self.instrument.index(),
        }
    }

    /// Apply edited settings *in place*, preserving live state (`last_freq`, the
    /// strobe phase) — unlike a full `State::default()` rebuild, which would drop
    /// the current readout. Re-maps the note so a reference/instrument change shows
    /// immediately.
    fn apply_config(&mut self, cfg: config::TunerConfig) {
        self.enabled = cfg.enabled;
        self.style = meter::MeterStyle::from_idx(cfg.meter_style_idx);
        self.a4_hz = cfg.a4_hz;
        self.instrument = Instrument::from_idx(cfg.instrument_idx);
        self.remap();
    }

    fn persist(&self) {
        overlay::save("tuner", &self.current_config());
    }
}

impl overlay::OverlayApp for State {
    type Message = Message;
    fn namespace() -> &'static str {
        "tuner"
    }
    #[cfg(target_os = "linux")]
    fn margin_changed(margin: (i32, i32, i32, i32)) -> Message {
        Message::MarginChange(margin)
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PitchUpdate(freq) => {
                self.last_freq = freq;
                self.remap();
            }
            Message::SetEnabled(on) => {
                self.enabled = on;
                self.persist();
            }
            Message::SetMeterStyle(s) => {
                self.style = s;
                self.persist();
            }
            Message::SetReference(hz) => {
                self.a4_hz = hz;
                self.remap();
                self.persist();
            }
            Message::SetInstrument(inst) => {
                self.instrument = inst;
                self.remap();
                self.persist();
            }
            Message::StrobeTick => {
                if let Some(n) = &self.note {
                    let speed = (n.cents.clamp(-50.0, 50.0) / 50.0) as f32; // -1.0..1.0
                    self.strobe_phase =
                        (self.strobe_phase + speed * 6.0).rem_euclid(meter::STROBE_BAND);
                }
            }
            Message::ReloadConfig => {
                // A watcher fires on every mtime bump, including our own persists.
                // Ignore a missing/malformed edit (don't reset to defaults), and
                // only apply when the file's settings actually differ from ours.
                if let Some(on_disk) = overlay::load_config_checked::<config::TunerConfig>("tuner")
                    && on_disk != self.current_config()
                {
                    self.apply_config(on_disk);
                }
            }
            Message::ResetDefaults => {
                self.apply_config(overlay::reset_defaults("tuner"));
            }
            // The `#[to_layer_message]` macro (Linux) adds variants (MarginChange, …)
            // this catch-all absorbs; off Linux the match is already exhaustive.
            #[cfg(target_os = "linux")]
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
        let Some(n) = &self.note else {
            return empty().into();
        };
        let [r, g, b] = pitch::cents_color(n.cents);
        let color = Color::from_rgb(r, g, b);
        let cents_label = if pitch::is_in_tune(n.cents) {
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
            container(body)
                .padding([6, 18])
                .style(|_theme| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba(
                        0.0, 0.0, 0.0, 0.45,
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
        let watch = Subscription::run(config_watch_stream);
        if self.enabled && self.style == meter::MeterStyle::Strobe {
            let ticks = Subscription::run(strobe_tick_stream);
            Subscription::batch([events, watch, ticks])
        } else {
            Subscription::batch([events, watch])
        }
    }
}

/// Emits `ReloadConfig` when the on-disk config changes — makes external edits
/// to `config.json` apply live (the only config surface on macOS, no tray).
fn config_watch_stream() -> BoxStream<'static, Message> {
    overlay::watch_config_stream("tuner", || Message::ReloadConfig)
}

fn event_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();

    // The tray is Linux-only (ksni / StatusNotifierItem over D-Bus). Off Linux the
    // overlay runs with the persisted config and no menu — see CLAUDE.md.
    #[cfg(target_os = "linux")]
    {
        let cfg: config::TunerConfig = overlay::load_config("tuner");
        ksni::TrayService::new(tray::TunerTray {
            tx: tx.clone(),
            enabled: cfg.enabled,
            style: meter::MeterStyle::from_idx(cfg.meter_style_idx),
            a4_hz: cfg.a4_hz,
            instrument: Instrument::from_idx(cfg.instrument_idx),
        })
        .spawn();
    }

    std::thread::spawn(move || audio::run(tx));

    Box::pin(rx)
}

fn strobe_tick_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(33));
            if tx.unbounded_send(Message::StrobeTick).is_err() {
                break;
            }
        }
    });
    Box::pin(rx)
}

fn main() -> overlay::Result {
    overlay::run::<State>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromatic_uses_nearest_note() {
        let n = note_from(440.0, 440.0, Instrument::Chromatic);
        assert_eq!((n.name, n.octave), ("A", 4));
        assert!(n.cents.abs() < 0.1, "cents {}", n.cents);
    }

    #[test]
    fn reference_shifts_chromatic_cents() {
        // 440 Hz read against a 442 reference is slightly flat.
        let n = note_from(440.0, 442.0, Instrument::Chromatic);
        assert_eq!(n.name, "A");
        assert!(n.cents < 0.0, "cents {}", n.cents);
    }

    #[test]
    fn guitar_preset_snaps_to_string() {
        // 110 Hz is the open A string → A2, in tune.
        let n = note_from(110.0, 440.0, Instrument::Guitar);
        assert_eq!((n.name, n.octave), ("A", 2));
        assert!(n.cents.abs() < 1.0, "cents {}", n.cents);
    }

    #[test]
    fn remap_follows_instrument_change() {
        let mut s = State {
            last_freq: Some(110.0),
            note: None,
            enabled: true,
            style: meter::MeterStyle::Needle,
            a4_hz: 440.0,
            instrument: Instrument::Chromatic,
            strobe_phase: 0.0,
        };
        s.remap();
        // Chromatic: 110 Hz is A2 (nearest note) too — check octave.
        assert_eq!(s.note.unwrap().octave, 2);
        s.instrument = Instrument::Guitar;
        s.remap();
        let n = s.note.unwrap();
        assert_eq!((n.name, n.octave), ("A", 2));
    }
}
