// The macOS build compiles without the tray; the menu/option helpers it would
// use are then unused. Silence dead-code there rather than cfg-gate each one.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use futures::channel::mpsc;
use futures::stream::BoxStream;
use iced::widget::{column, container, row, text};
use iced::{Color, Element, Subscription, Task};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod config;
mod exercise;
mod stats;
mod tone;
#[cfg(target_os = "linux")]
mod tray;
use config::VocalizeConfig;
use exercise::{Matcher, Mode, PlayStyle, Scale, ScaleKind};
use pitch::Note;
use tone::Timbre;

/// App name: Wayland namespace, single-instance lock, config dir.
const APP: &str = "vocalize";
/// Tall fixed panel (chips + readout), like karaoke owns its geometry.
const SURFACE_H: u32 = 160;
/// How long the "Acertou!" success flash shows before the next item.
const FLASH: Duration = Duration::from_millis(450);

/// Selectable scale roots (MIDI pitch classes 0–11), for the tray.
pub const ROOTS: [i64; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
/// Selectable cents-window steps.
pub const CENTS_STEPS: [f64; 3] = [25.0, 50.0, 75.0];
/// Selectable sustain-time steps (ms).
pub const SUSTAIN_STEPS: [f64; 3] = [300.0, 500.0, 800.0];

#[cfg_attr(target_os = "linux", iced_layershell::to_layer_message)]
#[derive(Debug, Clone)]
enum Message {
    SetEnabled(bool),
    SetAudible(bool),
    SetRoot(i64),
    SetScaleKind(exercise::ScaleKind),
    SetMode(exercise::Mode),
    SetPlayStyle(exercise::PlayStyle),
    SetTimbre(tone::Timbre),
    SetCents(f64),
    SetSustain(f64),
    /// Replay the current item's reference tone.
    Replay,
    /// Smoothed fundamental frequency (Hz) from the mic, or `None` on silence.
    PitchUpdate(Option<f64>),
    /// 33 ms UI tick driving listen/sustain timing and the success flash.
    Tick,
    /// The on-disk config changed (external edit); reload if it actually differs.
    ReloadConfig,
    /// Rewrite the config with defaults and rebuild from it (tray "Restaurar padrões").
    ResetDefaults,
    /// Clear the per-note practice statistics (tray "Limpar estatísticas").
    ResetStats,
}

struct State {
    enabled: bool,
    scale: Scale,
    mode: Mode,
    /// Whether chords play together (block) or as an arpejo.
    play_style: PlayStyle,
    /// Reference-tone timbre (electric piano or sine).
    timbre: Timbre,
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
    audible: bool,
    tone: tone::Tone,
    /// While `Some` and not elapsed, the reference tone is playing and the matcher
    /// is disabled (so the mic can't auto-pass on the tone bleeding in).
    present_until: Option<Instant>,
    /// Per-note practice statistics (time-to-sing per pitch class).
    stats: stats::Stats,
    /// When the current item became matchable (the present phase ended); used to
    /// measure how long each target takes to nail.
    item_armed_at: Instant,
}

impl Default for State {
    fn default() -> Self {
        let cfg: VocalizeConfig = overlay::load_or_seed(APP);
        let scale = Scale {
            root: cfg.scale_root,
            kind: ScaleKind::from_idx(cfg.scale_kind_idx),
        };
        let mode = Mode::from_idx(cfg.mode_idx);
        let play_style = PlayStyle::from_idx(cfg.play_style_idx);
        let timbre = Timbre::from_idx(cfg.timbre_idx);
        let cents_window = cfg.cents_window;
        let sustain_ms = cfg.sustain_ms as f64;
        let mut rng = seed();
        let degree = next_degree(&mut rng, scale.degree_count(), usize::MAX);
        let item = exercise::item_at(&scale, mode, degree);
        let matcher = Matcher::new(&item, cents_window, sustain_ms);
        let tone = tone::Tone::new(cfg.audible);
        let present = if cfg.enabled {
            tone.play(&freqs_of(&item), play_style == PlayStyle::Together, timbre)
        } else {
            std::time::Duration::ZERO
        };
        State {
            enabled: cfg.enabled,
            scale,
            mode,
            play_style,
            timbre,
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
            audible: cfg.audible,
            tone,
            present_until: Some(Instant::now() + present),
            stats: stats::load(),
            item_armed_at: Instant::now(),
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

/// Playback frequencies (Hz) for the item's MIDI notes, at A440.
fn freqs_of(item: &[i64]) -> Vec<f64> {
    item.iter()
        .map(|&m| pitch::note_to_frequency(m as f64, pitch::A4))
        .collect()
}

impl State {
    /// Move to a fresh random item and rebuild the matcher.
    fn advance(&mut self) {
        let degree = next_degree(&mut self.rng, self.scale.degree_count(), self.prev_degree);
        self.prev_degree = degree;
        self.item = exercise::item_at(&self.scale, self.mode, degree);
        self.matcher = Matcher::new(&self.item, self.cents_window, self.sustain_ms);
        let present = if self.enabled {
            self.tone.play(
                &freqs_of(&self.item),
                self.play_style == PlayStyle::Together,
                self.timbre,
            )
        } else {
            Duration::ZERO
        };
        self.present_until = Some(Instant::now() + present);
    }

    /// Apply a settings change by starting a fresh item under the new settings.
    fn reset(&mut self) {
        self.advance();
    }

    /// Apply edited settings *in place*, preserving the live exercise — unlike a
    /// full `State::default()` rebuild, which would jump to a new random target and
    /// blare the reference tone on every edit. Only a structural change
    /// (root/scale/mode) picks a fresh item, and it does so **silently** (a
    /// background file edit shouldn't sound the tone); scalar tweaks (tolerance,
    /// sustain, timbre, …) just re-arm the matcher on the current target.
    fn apply_config(&mut self, cfg: VocalizeConfig) {
        let structural = self.scale.root != cfg.scale_root
            || self.scale.kind.index() != cfg.scale_kind_idx
            || self.mode.index() != cfg.mode_idx;
        self.scale.root = cfg.scale_root;
        self.scale.kind = ScaleKind::from_idx(cfg.scale_kind_idx);
        self.mode = Mode::from_idx(cfg.mode_idx);
        self.play_style = PlayStyle::from_idx(cfg.play_style_idx);
        self.timbre = Timbre::from_idx(cfg.timbre_idx);
        self.cents_window = cfg.cents_window;
        self.sustain_ms = cfg.sustain_ms as f64;
        self.audible = cfg.audible;
        self.tone.set_audible(cfg.audible);
        self.enabled = cfg.enabled;
        if structural {
            // The target depends on the scale/mode; pick a fresh item, but don't
            // sound the tone (this is a silent config edit, not a user action).
            self.prev_degree = usize::MAX;
            let degree = next_degree(&mut self.rng, self.scale.degree_count(), self.prev_degree);
            self.prev_degree = degree;
            self.item = exercise::item_at(&self.scale, self.mode, degree);
            self.present_until = None;
            // A silent structural change arms a fresh target immediately.
            self.item_armed_at = Instant::now();
        }
        // Re-arm the matcher for the (possibly new) item under the new tolerance.
        self.matcher = Matcher::new(&self.item, self.cents_window, self.sustain_ms);
    }

    /// The current settings as a serializable config (for persisting / comparing).
    fn current_config(&self) -> VocalizeConfig {
        VocalizeConfig {
            enabled: self.enabled,
            audible: self.audible,
            scale_root: self.scale.root,
            scale_kind_idx: self.scale.kind.index(),
            mode_idx: self.mode.index(),
            play_style_idx: self.play_style.index(),
            timbre_idx: self.timbre.index(),
            cents_window: self.cents_window,
            sustain_ms: self.sustain_ms as u64,
        }
    }

    fn persist(&self) {
        overlay::save(APP, &self.current_config());
    }
}

impl overlay::OverlayApp for State {
    type Message = Message;
    fn namespace() -> &'static str {
        APP
    }
    #[cfg(target_os = "linux")]
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
                self.last_tick = Instant::now();
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
                if let Some(t) = self.present_until {
                    if now < t {
                        return Task::none();
                    }
                    self.present_until = None;
                    // The item is now matchable: start the time-to-sing clock.
                    self.item_armed_at = now;
                }
                let newly = self.matcher.update(self.sung, dt);
                if !newly.is_empty() {
                    // Record how long each just-collected target took to nail.
                    let elapsed_ms = (now - self.item_armed_at).as_secs_f64() * 1000.0;
                    for &i in &newly {
                        self.stats.record(self.item[i].rem_euclid(12), elapsed_ms);
                    }
                    stats::save(&self.stats);
                    if self.matcher.all_collected() {
                        self.success_until = Some(now + FLASH);
                    }
                }
            }
            Message::SetAudible(on) => {
                self.audible = on;
                self.tone.set_audible(on);
                self.persist();
            }
            Message::SetRoot(r) => {
                self.scale.root = r;
                self.persist();
                self.reset();
            }
            Message::SetScaleKind(k) => {
                self.scale.kind = k;
                self.prev_degree = usize::MAX;
                self.persist();
                self.reset();
            }
            Message::SetMode(m) => {
                self.mode = m;
                self.persist();
                self.reset();
            }
            Message::SetPlayStyle(s) => {
                self.play_style = s;
                self.persist();
                self.reset();
            }
            Message::SetTimbre(t) => {
                self.timbre = t;
                self.persist();
                self.reset();
            }
            Message::SetCents(c) => {
                self.cents_window = c;
                self.persist();
                self.reset();
            }
            Message::SetSustain(ms) => {
                self.sustain_ms = ms;
                self.persist();
                self.reset();
            }
            Message::Replay if self.enabled => {
                let present = self.tone.play(
                    &freqs_of(&self.item),
                    self.play_style == PlayStyle::Together,
                    self.timbre,
                );
                self.present_until = Some(Instant::now() + present);
            }
            Message::ReloadConfig => {
                // A watcher fires on every mtime bump, including our own persists.
                // Ignore a missing/malformed edit (don't reset to defaults), and
                // only apply when the file's settings actually differ from ours.
                if let Some(on_disk) = overlay::load_config_checked::<VocalizeConfig>(APP)
                    && on_disk != self.current_config()
                {
                    self.apply_config(on_disk);
                }
            }
            Message::ResetDefaults => {
                self.apply_config(overlay::reset_defaults(APP));
            }
            Message::ResetStats => {
                self.stats.clear();
                stats::save(&self.stats);
            }
            // Absorbs the guarded `Replay` (when disabled) plus, on Linux, the extra
            // variants the `#[to_layer_message]` macro injects (MarginChange, …).
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
        let prompt = if flashing {
            "Acertou!"
        } else if self.present_until.is_some() {
            "Ouça…"
        } else {
            "Cante:"
        };
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
        let mut body = column![
            text(prompt).size(18.0).color(Color::WHITE),
            chips,
            text(you_label).size(16.0).color(you_color),
        ]
        .align_x(iced::Center)
        .spacing(8);

        // Per-note practice hint: the pitch class you take longest to nail.
        if let Some((class, avg_ms)) = self.stats.hardest() {
            body = body.push(
                text(format!(
                    "⌛ mais difícil: {} (~{:.1}s)",
                    exercise::note_label(60 + class as i64),
                    avg_ms / 1000.0
                ))
                .size(13.0)
                .color(Color::from_rgba(1.0, 0.85, 0.4, 0.8)),
            );
        }
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
        let watch = Subscription::run(config_watch_stream);
        if self.enabled {
            Subscription::batch([events, watch, Subscription::run(tick_stream)])
        } else {
            Subscription::batch([events, watch])
        }
    }
}

/// Emits `ReloadConfig` when the on-disk config changes — makes external edits
/// to `config.json` apply live (the only config surface on macOS, no tray).
fn config_watch_stream() -> BoxStream<'static, Message> {
    overlay::watch_config_stream(APP, || Message::ReloadConfig)
}

fn event_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();

    // The tray is Linux-only (ksni / StatusNotifierItem over D-Bus). Off Linux the
    // overlay runs with the persisted config and no menu — see CLAUDE.md.
    #[cfg(target_os = "linux")]
    {
        let cfg: VocalizeConfig = overlay::load_config(APP);
        ksni::TrayService::new(tray::VocalizeTray {
            tx: tx.clone(),
            enabled: cfg.enabled,
            audible: cfg.audible,
            scale_root: cfg.scale_root,
            scale_kind: ScaleKind::from_idx(cfg.scale_kind_idx),
            mode: Mode::from_idx(cfg.mode_idx),
            play_style: PlayStyle::from_idx(cfg.play_style_idx),
            timbre: Timbre::from_idx(cfg.timbre_idx),
            cents_window: cfg.cents_window,
            sustain_ms: cfg.sustain_ms as f64,
        })
        .spawn();
    }

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

fn main() -> overlay::Result {
    overlay::run::<State>()
}
