# Vocal Tuner App — Implementation Plan (Plan 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Add `crates/tuner` — a Wayland overlay that shows the musical note being sung into the microphone (note name + cents + a tuning meter), built on the shared `overlay` library.

**Architecture:** A new binary crate implementing `overlay::OverlayApp`. An audio thread captures the default input device (`cpal`) into a sample buffer; an analysis loop runs pitch detection (`pitch-detection`, McLeod method) ~20×/s, converts frequency → nearest note + cents, and pushes `PitchUpdate` messages into the iced subscription. The `view` renders the note and a horizontal meter (green when in tune). No tray yet (the generic tray is deferred to a later plan); quit via Ctrl-C / kill for now.

**Tech Stack:** Rust, `overlay` crate (this workspace), `iced` 0.14, `iced_layershell` 0.18, `cpal` (audio in), `pitch-detection` (fundamental frequency).

**Reference:** spec `docs/specs/2026-06-12-vocal-tuner-design.md`. The locked base API is in `crates/overlay/src/lib.rs`:

```rust
pub trait OverlayApp: Default + Sized + 'static {
    type Message: Clone + std::fmt::Debug + Send + 'static
        + TryInto<iced_layershell::actions::LayerShellCustomActionWithId, Error = Self::Message>;
    fn namespace() -> &'static str;
    fn update(&mut self, message: Self::Message) -> iced::Task<Self::Message>;
    fn view(&self) -> iced::Element<'_, Self::Message>;
    fn subscription(&self) -> iced::Subscription<Self::Message>;
}
pub fn run<A: OverlayApp>() -> iced_layershell::Result;
```

`lyrics` is the reference implementor (`crates/lyrics/src/main.rs`): a `State` (Default), a `#[to_layer_message] enum Message`, `impl OverlayApp for State`, `main() { overlay::run::<State>() }`, and a subscription that bridges a background thread via `futures::channel::mpsc::unbounded` + `BoxStream`. Mirror that shape.

---

## File Structure (target)

```
crates/tuner/
  Cargo.toml
  src/
    main.rs        # State/Message/impl OverlayApp + main(); the mpsc→subscription bridge
    note.rs        # pure: frequency_to_note, cents, meter_position, is_in_tune (fully unit-tested)
    audio.rs       # cpal capture + pitch-detection analysis thread → emits Message::PitchUpdate
```

---

## Task 1: Scaffold the tuner crate on the overlay base (runs blank)

Goal: a `tuner` binary that compiles, runs via `overlay::run`, and shows a placeholder — proving the base wiring before any audio.

**Files:**
- Create: `crates/tuner/Cargo.toml`
- Create: `crates/tuner/src/main.rs`
- Create: `crates/tuner/src/note.rs` (just the `Note` type for now)

- [ ] **Step 1: `crates/tuner/Cargo.toml`**

```toml
[package]
name = "tuner"
version.workspace = true
edition.workspace = true

[dependencies]
overlay = { path = "../overlay" }
iced.workspace = true
iced_layershell.workspace = true
futures.workspace = true
```

- [ ] **Step 2: `crates/tuner/src/note.rs` — the Note type**

```rust
//! Pure pitch → note math (no audio, no UI). Fully unit-tested.

/// A detected note: name, octave, and cents deviation from perfect pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub name: &'static str,
    pub octave: i32,
    pub cents: f64,
}
```

- [ ] **Step 3: `crates/tuner/src/main.rs` — skeleton implementing OverlayApp**

```rust
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

// Placeholder to keep BoxStream/mpsc imports used until Task 4 wires audio.
#[allow(dead_code)]
fn _unused() -> BoxStream<'static, Message> {
    let (_tx, rx) = mpsc::unbounded::<Message>();
    Box::pin(rx)
}
```

- [ ] **Step 4: Build**

Run: `cargo build -p tuner`
Expected: builds clean. (Do not run the GUI here — no display/audio in CI; the owner runs `./target/debug/tuner` in their session.)

Run: `cargo clippy -p tuner --all-targets`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(tuner): scaffold tuner crate on the overlay base"
```

---

## Task 2: Pure pitch → note math (TDD)

Goal: `frequency_to_note`, `meter_position`, `is_in_tune` with exhaustive tests. This is the deterministic core.

**Files:**
- Modify: `crates/tuner/src/note.rs`

- [ ] **Step 1: Write the failing tests** (append to `note.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) -> bool { (a - b).abs() < eps }

    #[test]
    fn a4_is_440() {
        let n = frequency_to_note(440.0, 440.0);
        assert_eq!(n.name, "A");
        assert_eq!(n.octave, 4);
        assert!(approx(n.cents, 0.0, 0.01), "cents {}", n.cents);
    }

    #[test]
    fn middle_c_is_c4() {
        let n = frequency_to_note(261.626, 440.0);
        assert_eq!(n.name, "C");
        assert_eq!(n.octave, 4);
        assert!(approx(n.cents, 0.0, 1.0), "cents {}", n.cents);
    }

    #[test]
    fn a_sharp_4() {
        let n = frequency_to_note(466.164, 440.0);
        assert_eq!(n.name, "A#");
        assert_eq!(n.octave, 4);
    }

    #[test]
    fn slightly_sharp_is_positive_cents() {
        let n = frequency_to_note(445.0, 440.0);
        assert_eq!(n.name, "A");
        assert!(n.cents > 0.0 && n.cents < 50.0, "cents {}", n.cents);
    }

    #[test]
    fn meter_position_maps_range() {
        assert!(approx(meter_position(0.0), 0.5, 1e-9));
        assert!(approx(meter_position(50.0), 1.0, 1e-9));
        assert!(approx(meter_position(-50.0), 0.0, 1e-9));
        assert!(approx(meter_position(100.0), 1.0, 1e-9)); // clamped
    }

    #[test]
    fn in_tune_threshold() {
        assert!(is_in_tune(3.0));
        assert!(is_in_tune(-4.9));
        assert!(!is_in_tune(10.0));
    }
}
```

- [ ] **Step 2: Run tests, verify they FAIL**

Run: `cargo test -p tuner`
Expected: FAIL (functions not defined).

- [ ] **Step 3: Implement** (add to `note.rs`, above the tests)

```rust
const NAMES: [&str; 12] = ["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"];

/// Nearest note to `freq` (Hz), with reference `a4` (usually 440.0).
pub fn frequency_to_note(freq: f64, a4: f64) -> Note {
    let midi = 69.0 + 12.0 * (freq / a4).log2();
    let rounded = midi.round();
    let cents = (midi - rounded) * 100.0;
    let semitone = rounded as i64;
    let name = NAMES[semitone.rem_euclid(12) as usize];
    let octave = (semitone.div_euclid(12) - 1) as i32; // MIDI: note 0 = C-1
    Note { name, octave, cents }
}

/// Map cents in [-50, 50] to a meter position in [0.0, 1.0] (0.5 = in tune).
pub fn meter_position(cents: f64) -> f64 {
    ((cents / 50.0).clamp(-1.0, 1.0) + 1.0) / 2.0
}

/// Whether the deviation is small enough to call "in tune".
pub fn is_in_tune(cents: f64) -> bool {
    cents.abs() < 5.0
}
```

- [ ] **Step 4: Run tests, verify PASS**

Run: `cargo test -p tuner`
Expected: PASS (6 new tests green).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(tuner): pure frequency→note + meter math (TDD)"
```

---

## Task 3: Render the note + tuning meter in `view`

Goal: replace the placeholder `view` with the approved layout — note name on top, a horizontal meter with a marker, cents below, green when in tune.

**Files:**
- Modify: `crates/tuner/src/main.rs`

- [ ] **Step 1: Implement the view** (replace the `view` method body)

```rust
fn view(&self) -> Element<'_, Message> {
    use iced::widget::{column, container, progress_bar, text};
    let content: Element<'_, Message> = match &self.note {
        None => text("").into(),
        Some(n) => {
            let in_tune = note::is_in_tune(n.cents);
            let pos = note::meter_position(n.cents) as f32;
            let color = if in_tune { Color::from_rgb(0.3, 0.9, 0.3) } else { Color::WHITE };
            column![
                text(format!("{}{}", n.name, n.octave)).size(44.0).color(color),
                progress_bar(0.0..=1.0, pos).height(8.0),
                text(format!("{:+.0}¢", n.cents)).size(20.0).color(color),
            ]
            .align_x(iced::Center)
            .spacing(4)
            .into()
        }
    };
    container(content).center_x(iced::Fill).center_y(iced::Fill).into()
}
```

(If `progress_bar`'s exact 0.14 signature differs, adapt to the version's API — the
intent is a horizontal bar whose fill tracks `pos`, 0.5 = centered/in tune.)

- [ ] **Step 2: Build + clippy**

Run: `cargo build -p tuner && cargo clippy -p tuner --all-targets`
Expected: clean. (UI is verified manually by the owner.)

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(tuner): render note + tuning meter"
```

---

## Task 4: Audio capture + pitch detection (spike) → PitchUpdate stream

Goal: capture the default mic with `cpal`, detect pitch with `pitch-detection`, and feed `Message::PitchUpdate(Option<Note>)` into the iced subscription. Includes a deterministic detector test on a synthetic sine wave.

**This is a spike:** cpal device/format negotiation and `!Send` streams are environment-dependent. You have latitude to adjust the exact cpal setup to compile and work on Linux (ALSA/PipeWire, f32 input). Do NOT block the analysis on the audio realtime thread.

**Files:**
- Modify: `crates/tuner/Cargo.toml` (add deps)
- Create: `crates/tuner/src/audio.rs`
- Modify: `crates/tuner/src/main.rs` (wire the subscription; remove the `_unused` placeholder)

- [ ] **Step 1: Add dependencies to `crates/tuner/Cargo.toml`**

```toml
cpal = "0.15"
pitch-detection = "0.3"
```

(If those exact versions are unavailable, pick the current compatible release and note it.)

- [ ] **Step 2: Write the failing detector test** in `crates/tuner/src/audio.rs`

```rust
//! Microphone capture + pitch detection feeding Message::PitchUpdate.

use crate::note::{frequency_to_note, Note};

/// Estimate the fundamental frequency of a mono f32 buffer, or `None` if no
/// clear pitch (clarity below `min_clarity`). `sample_rate` in Hz.
pub fn detect_frequency(samples: &[f32], sample_rate: u32, min_clarity: f64) -> Option<f64> {
    // implemented in Step 4
    let _ = (samples, sample_rate, min_clarity);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine(freq: f32, sr: u32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (2.0 * PI * freq * i as f32 / sr as f32).sin()).collect()
    }

    #[test]
    fn detects_a440_sine() {
        let sr = 44_100;
        let buf = sine(440.0, sr, 4096);
        let f = detect_frequency(&buf, sr, 0.5).expect("should detect a clear sine");
        assert!((f - 440.0).abs() < 5.0, "got {f}");
    }

    #[test]
    fn silence_has_no_pitch() {
        let buf = vec![0.0f32; 4096];
        assert!(detect_frequency(&buf, 44_100, 0.5).is_none());
    }

    #[test]
    fn maps_detected_freq_to_note() {
        let n: Note = frequency_to_note(440.0, 440.0);
        assert_eq!((n.name, n.octave), ("A", 4));
    }
}
```

- [ ] **Step 3: Run the test, verify it FAILS**

Run: `cargo test -p tuner detect`
Expected: FAIL (`detects_a440_sine` — returns None).

- [ ] **Step 4: Implement `detect_frequency`** using `pitch-detection`'s McLeod detector

```rust
use pitch_detection::detector::mcleod::McLeodDetector;
use pitch_detection::detector::PitchDetector;

pub fn detect_frequency(samples: &[f32], sample_rate: u32, min_clarity: f64) -> Option<f64> {
    let size = samples.len();
    if size < 256 { return None; }
    let padding = size / 2;
    let signal: Vec<f64> = samples.iter().map(|&s| s as f64).collect();
    let mut detector = McLeodDetector::new(size, padding);
    detector
        .get_pitch(&signal, sample_rate as usize, /*power_threshold*/ 0.15, min_clarity)
        .map(|p| p.frequency)
}
```

(Adjust constructor/threshold args to the crate's actual `get_pitch` signature; the
intent is: return the fundamental only when power and clarity clear the thresholds.)

- [ ] **Step 5: Run tests, verify PASS**

Run: `cargo test -p tuner`
Expected: PASS — all `note` tests + the 3 `audio` tests green.

- [ ] **Step 6: Implement the capture + analysis runner** in `audio.rs`

```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use futures::channel::mpsc::UnboundedSender;
use std::sync::{Arc, Mutex};
use crate::Message;

const A4: f64 = 440.0;
const WINDOW: usize = 4096;     // samples per analysis
const MIN_CLARITY: f64 = 0.6;

/// Owns the cpal input stream + an analysis loop; sends PitchUpdate forever.
pub fn run(tx: UnboundedSender<Message>) {
    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
        eprintln!("[tuner] no input device"); return;
    };
    let Ok(cfg) = device.default_input_config() else {
        eprintln!("[tuner] no input config"); return;
    };
    let sample_rate = cfg.sample_rate().0;
    let channels = cfg.channels() as usize;

    // Shared ring of the last WINDOW mono samples.
    let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(WINDOW)));
    let cb_buf = buf.clone();

    // Build an f32 input stream (assume f32; adapt if the device differs).
    let err_fn = |e| eprintln!("[tuner] stream error: {e}");
    let stream = device.build_input_stream(
        &cfg.into(),
        move |data: &[f32], _| {
            // Downmix to mono by taking the first channel of each frame.
            let mut b = cb_buf.lock().unwrap();
            for frame in data.chunks(channels) {
                b.push(frame[0]);
            }
            let len = b.len();
            if len > WINDOW { b.drain(0..len - WINDOW); }
        },
        err_fn,
        None,
    );
    let Ok(stream) = stream else { eprintln!("[tuner] build_input_stream failed"); return; };
    if stream.play().is_err() { eprintln!("[tuner] stream.play failed"); return; }

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let window: Vec<f32> = { buf.lock().unwrap().clone() };
        let note = (window.len() >= WINDOW)
            .then(|| super::audio::detect_frequency(&window, sample_rate, MIN_CLARITY))
            .flatten()
            .map(|f| crate::note::frequency_to_note(f, A4));
        if tx.unbounded_send(Message::PitchUpdate(note)).is_err() {
            break; // app exited
        }
    }
    // keep `stream` alive for the loop's lifetime
}
```

- [ ] **Step 7: Wire the subscription in `main.rs`**

Remove the `_unused` placeholder. Replace the `subscription` method:

```rust
fn subscription(&self) -> Subscription<Message> {
    Subscription::run(audio_stream)
}
```

Add the stream bridge (mirrors the lyrics `event_stream`):

```rust
mod audio;

fn audio_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    std::thread::spawn(move || audio::run(tx));
    Box::pin(rx)
}
```

- [ ] **Step 8: Build, test, clippy**

Run: `cargo test -p tuner` → all green.
Run: `cargo build -p tuner` → clean.
Run: `cargo clippy -p tuner --all-targets` → no warnings.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(tuner): mic capture + pitch detection → note stream"
```

---

## Task 5: Workspace-wide verification

Goal: confirm the whole workspace is healthy and document how to run the tuner.

**Files:** none (verification only).

- [ ] **Step 1: Full workspace checks**

Run: `cargo test` → all crates green (lyrics 50, overlay 5, tuner ~9).
Run: `cargo clippy --all-targets` → no warnings.
Run: `cargo build --release` → both `lyrics` and `tuner` binaries produced.
Run: `ls target/release/tuner target/release/lyrics`.

- [ ] **Step 2: Commit if anything changed; otherwise done.**

Owner verification (manual, in a real session): run `./target/release/tuner`, sing/hum
a steady note → the note name + meter track pitch, meter centers/greens when in tune,
goes blank on silence.

---

## Self-Review (done while writing)

- **Spec coverage:** capture (`cpal`, Task 4), detection (`pitch-detection`, Task 4),
  freq→note+cents (Task 2), gate/hide on silence (Task 4 `None` path + Task 3 empty view),
  meter + green-in-tune (Task 3), A=440 / sharps (Task 2 `NAMES`/`A4`), sine-wave detector
  test (Task 4), deterministic note tests (Task 2). Tray intentionally deferred (noted in
  spec/architecture).
- **Placeholder note:** Task 4 cpal/`get_pitch` exact signatures are flagged as spike details
  resolved against the crate/compiler; every other step has exact, runnable code.
- **Type consistency:** `Note`, `Message::PitchUpdate(Option<Note>)`, `frequency_to_note`,
  `meter_position`, `is_in_tune`, `detect_frequency`, `audio::run`, `audio_stream` are used
  consistently across tasks.
