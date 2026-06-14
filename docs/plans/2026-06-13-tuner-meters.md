# Tuner Meters, Tray, and Smoothing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the tuner a real, good-looking display — three selectable meter styles (Needle default, CenterBar, Strobe) on a translucent pill, gradient color feedback, anti-jitter smoothing, and a tray to switch/persist the style.

**Architecture:** Pure, testable cores first (smoother, color, style enum, config), then iced `Canvas`-based rendering, a ksni tray, and a gated animation tick for the strobe. Config I/O in `overlay` is generalized so persistence isn't duplicated across apps. Pitch detection stays on the existing `pitch-detection` (McLeod/MPM) crate.

**Tech Stack:** Rust (edition 2024), `iced` 0.14 (+ `canvas` feature) under `iced_layershell`, `ksni` tray, `serde`, `pitch-detection`, `cpal`.

Spec: `docs/specs/2026-06-13-tuner-meters-design.md`. Verify with the **1.96 toolchain** (`cargo +stable …`) since CI denies warnings on 1.96.

---

### Task 1: Generalize overlay config I/O; move `SavedConfig` to lyrics

Make `overlay::load_config`/`save` generic over any serde type so each app owns its config struct (no duplicated JSON logic). Move the lyrics-specific `SavedConfig` into the `lyrics` crate. `FontSize`/`Position` stay in `overlay`.

**Files:**
- Modify: `crates/overlay/src/settings.rs` (remove `SavedConfig` + its defaults/tests; make I/O generic)
- Modify: `crates/overlay/src/lib.rs` (drop `SavedConfig` from re-exports)
- Create: `crates/lyrics/src/config.rs` (`SavedConfig` moves here, with its tests)
- Modify: `crates/lyrics/src/main.rs` (add `mod config`; update call sites)

- [ ] **Step 1: Make `overlay` config I/O generic**

In `crates/overlay/src/settings.rs`, delete the `// ── SavedConfig ──` block (the `SavedConfig` struct, `default_font_idx`, `default_enabled`, and `impl Default for SavedConfig`) AND the three `SavedConfig` tests in its `mod tests` (`saved_config_roundtrips_json`, `saved_config_missing_fields_use_defaults`, `saved_config_ignores_legacy_mode_idx`). Keep `FontSize`, `Position`, and the two `fontsize_*` tests. Remove the now-unused `use serde::{Deserialize, Serialize};` at the top.

Replace the `load_config`/`save` functions with generic versions (keep `config_path` as-is):

```rust
pub fn load_config<T: Default + serde::de::DeserializeOwned>(app: &str) -> T {
    config_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save<T: serde::Serialize>(app: &str, cfg: &T) {
    if cfg!(test) {
        return;
    }
    let Some(path) = config_path(app) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(cfg) {
        let _ = std::fs::write(path, json);
    }
}
```

- [ ] **Step 2: Drop `SavedConfig` from overlay re-exports**

In `crates/overlay/src/lib.rs`, change:

```rust
pub use settings::{FontSize, Position, SavedConfig, load_config, save};
```

to:

```rust
pub use settings::{FontSize, Position, load_config, save};
```

- [ ] **Step 3: Create `crates/lyrics/src/config.rs` with `SavedConfig` + its tests**

```rust
//! Persisted lyrics settings (font size + enabled), stored as JSON via `overlay`.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct SavedConfig {
    #[serde(default = "default_font_idx")]
    pub font_size_idx: usize,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_font_idx() -> usize {
    1
}
fn default_enabled() -> bool {
    true
}

impl Default for SavedConfig {
    fn default() -> Self {
        SavedConfig {
            font_size_idx: 1,
            enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_config_roundtrips_json() {
        let cfg = SavedConfig {
            font_size_idx: 2,
            enabled: false,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: SavedConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.font_size_idx, 2);
        assert!(!loaded.enabled);
    }

    #[test]
    fn saved_config_missing_fields_use_defaults() {
        let cfg: SavedConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.font_size_idx, 1);
        assert!(cfg.enabled);
    }

    #[test]
    fn saved_config_ignores_legacy_mode_idx() {
        let cfg: SavedConfig =
            serde_json::from_str(r#"{"font_size_idx":2,"mode_idx":1,"enabled":true}"#).unwrap();
        assert_eq!(cfg.font_size_idx, 2);
        assert!(cfg.enabled);
    }
}
```

- [ ] **Step 4: Wire lyrics to its own `SavedConfig` + the generic API**

In `crates/lyrics/src/main.rs`:

1. Add the module near the other `mod`/`use` lines at the top (after the existing `use` block):

```rust
mod config;
use config::SavedConfig;
```

2. In `impl Default for State`, annotate the load so type inference resolves:

```rust
        let cfg: SavedConfig = overlay::load_config(APP);
```

3. In `event_stream`, annotate likewise:

```rust
    let cfg: SavedConfig = overlay::load_config(APP);
```

4. The two `overlay::save(APP, state.font_size, state.enabled);` call sites (in `update`) become:

```rust
            overlay::save(
                APP,
                &SavedConfig {
                    font_size_idx: state.font_size.index(),
                    enabled: state.enabled,
                },
            );
```

(Both call sites are identical — replace each.)

- [ ] **Step 5: Verify and commit**

Run: `cargo +stable build --workspace 2>&1 | tail -5 && cargo +stable test --workspace 2>&1 | grep -E "test result:|error\[" && cargo +stable clippy --all-targets -- -D warnings 2>&1 | tail -3`
Expected: build OK; clippy no warnings; tests `0 failed`. Counts: **overlay 7** (was 10, −3 moved SavedConfig tests), **lyrics 54** (was 51, +3 moved), **tuner 8** (unchanged).

```bash
git add crates/overlay/src/settings.rs crates/overlay/src/lib.rs crates/lyrics/src/config.rs crates/lyrics/src/main.rs
git commit -m "refactor(overlay): generic typed config I/O; move SavedConfig into lyrics

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Pitch smoother (EMA + hold) — pure & tested

**Files:**
- Create: `crates/tuner/src/smooth.rs`
- Modify: `crates/tuner/src/main.rs` (add `mod smooth;`)

- [ ] **Step 1: Write the smoother + its tests**

Create `crates/tuner/src/smooth.rs`:

```rust
//! EMA smoothing + a short "hold" for the detected pitch, kept pure for testing.

/// EMA factor (0..1; higher = snappier, lower = smoother).
const ALPHA: f64 = 0.25;
/// Consecutive `None` analysis frames tolerated before the pitch is dropped.
/// At ~50 ms/frame, 6 frames ≈ 300 ms.
const HOLD_FRAMES: u32 = 6;

/// Smooths a stream of raw per-frame frequency estimates.
#[derive(Default)]
pub struct Smoother {
    ema: Option<f64>,
    misses: u32,
}

impl Smoother {
    /// Feed the latest raw frequency (`None` = no clear pitch this frame).
    /// Returns the smoothed frequency to display, or `None` once the hold
    /// window is exceeded. Resets after a real silence so the next onset is
    /// not dragged toward the old pitch.
    pub fn update(&mut self, raw: Option<f64>) -> Option<f64> {
        match raw {
            Some(f) => {
                self.misses = 0;
                let next = match self.ema {
                    Some(prev) => ALPHA * f + (1.0 - ALPHA) * prev,
                    None => f,
                };
                self.ema = Some(next);
                Some(next)
            }
            None => {
                self.misses += 1;
                if self.misses > HOLD_FRAMES {
                    self.ema = None;
                    None
                } else {
                    self.ema
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_reading_passes_through() {
        let mut s = Smoother::default();
        assert_eq!(s.update(Some(440.0)), Some(440.0));
    }

    #[test]
    fn ema_blends_subsequent_readings() {
        let mut s = Smoother::default();
        s.update(Some(440.0));
        let v = s.update(Some(450.0)).unwrap();
        assert!((v - 442.5).abs() < 1e-9, "got {v}");
    }

    #[test]
    fn holds_last_value_then_drops() {
        let mut s = Smoother::default();
        s.update(Some(440.0));
        for _ in 0..HOLD_FRAMES {
            assert_eq!(s.update(None), Some(440.0));
        }
        assert_eq!(s.update(None), None);
    }

    #[test]
    fn restarts_cold_after_drop() {
        let mut s = Smoother::default();
        s.update(Some(440.0));
        for _ in 0..=HOLD_FRAMES {
            s.update(None);
        }
        assert_eq!(s.update(Some(300.0)), Some(300.0));
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/tuner/src/main.rs`, add to the module list near `mod audio; mod note;`:

```rust
mod smooth;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo +stable test -p tuner smooth 2>&1 | tail -10`
Expected: PASS — 4 `smooth::tests`.

- [ ] **Step 4: Commit**

```bash
git add crates/tuner/src/smooth.rs crates/tuner/src/main.rs
git commit -m "feat(tuner): EMA + hold pitch smoother

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Wire the smoother into the audio loop

**Files:**
- Modify: `crates/tuner/src/audio.rs` (analysis loop)

- [ ] **Step 1: Smooth the raw frequency before mapping to a note**

In `crates/tuner/src/audio.rs`, find the analysis `loop` (it computes `let note = (window.len() >= WINDOW).then(|| super::detect_frequency(...)).flatten().map(|f| crate::note::frequency_to_note(f, A4));` then sends `Message::PitchUpdate(note)`).

Just before the `loop {`, create the smoother:

```rust
        let mut smoother = crate::smooth::Smoother::default();
```

Inside the loop, replace the `let note = …;` computation with:

```rust
            let raw = (window.len() >= WINDOW)
                .then(|| super::detect_frequency(&window, sample_rate, MIN_CLARITY))
                .flatten();
            let note = smoother
                .update(raw)
                .map(|f| crate::note::frequency_to_note(f, A4));
```

Leave the `overlay::debug(...)` note trace and the `tx.unbounded_send(Message::PitchUpdate(note))` below it unchanged.

- [ ] **Step 2: Verify build/tests/clippy**

Run: `cargo +stable build -p tuner 2>&1 | tail -3 && cargo +stable test -p tuner 2>&1 | grep -E "test result:" && cargo +stable clippy -p tuner --all-targets -- -D warnings 2>&1 | tail -3`
Expected: build OK; tests `0 failed` (12: 8 prior + 4 smooth); clippy clean.

- [ ] **Step 3: Commit**

```bash
git add crates/tuner/src/audio.rs
git commit -m "feat(tuner): feed mic pitch through the smoother

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `MeterStyle` enum + `cents_color` — pure & tested

**Files:**
- Create: `crates/tuner/src/meter.rs`
- Modify: `crates/tuner/src/main.rs` (add `mod meter;`)

- [ ] **Step 1: Write the enum + color helper + tests**

Create `crates/tuner/src/meter.rs`:

```rust
//! Tuning-meter styles and color feedback. Canvas drawing is added later.

use iced::Color;

/// Width (px) of one strobe band; shared by the drawing and the animation step.
pub const STROBE_BAND: f32 = 24.0;

/// The visual style of the tuning meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MeterStyle {
    #[default]
    Needle,
    CenterBar,
    Strobe,
}

impl MeterStyle {
    pub fn index(self) -> usize {
        match self {
            MeterStyle::Needle => 0,
            MeterStyle::CenterBar => 1,
            MeterStyle::Strobe => 2,
        }
    }

    pub fn from_idx(i: usize) -> Self {
        match i {
            1 => MeterStyle::CenterBar,
            2 => MeterStyle::Strobe,
            _ => MeterStyle::Needle,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MeterStyle::Needle => "Agulha",
            MeterStyle::CenterBar => "Barra",
            MeterStyle::Strobe => "Strobe",
        }
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Green within ±5¢, blending to amber by ±25¢ and red by ±50¢ (symmetric).
pub fn cents_color(cents: f64) -> Color {
    const GREEN: [f32; 3] = [0.30, 0.90, 0.30];
    const AMBER: [f32; 3] = [0.95, 0.75, 0.20];
    const RED: [f32; 3] = [0.90, 0.25, 0.25];
    let c = cents.abs();
    let rgb = if c <= 5.0 {
        GREEN
    } else if c <= 25.0 {
        lerp(GREEN, AMBER, ((c - 5.0) / 20.0) as f32)
    } else {
        lerp(AMBER, RED, (((c - 25.0) / 25.0).min(1.0)) as f32)
    };
    Color::from_rgb(rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_index_roundtrips() {
        for s in [MeterStyle::Needle, MeterStyle::CenterBar, MeterStyle::Strobe] {
            assert_eq!(MeterStyle::from_idx(s.index()), s);
        }
    }

    #[test]
    fn style_unknown_idx_defaults_to_needle() {
        assert_eq!(MeterStyle::from_idx(99), MeterStyle::Needle);
    }

    #[test]
    fn in_tune_is_green() {
        let g = cents_color(0.0);
        assert!((g.g - 0.90).abs() < 1e-6 && g.r < 0.4, "{g:?}");
        assert_eq!(cents_color(4.9), cents_color(0.0));
    }

    #[test]
    fn far_out_is_red_and_clamped() {
        let r = cents_color(50.0);
        assert!(r.r > 0.85 && r.g < 0.3, "{r:?}");
        assert_eq!(cents_color(80.0), cents_color(50.0));
    }

    #[test]
    fn color_is_symmetric() {
        let a = cents_color(-20.0);
        let b = cents_color(20.0);
        assert!((a.r - b.r).abs() < 1e-6 && (a.g - b.g).abs() < 1e-6);
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/tuner/src/main.rs`, add to the module list:

```rust
mod meter;
```

- [ ] **Step 3: Run tests**

Run: `cargo +stable test -p tuner meter 2>&1 | tail -10`
Expected: PASS — 5 `meter::tests`.

- [ ] **Step 4: Commit**

```bash
git add crates/tuner/src/meter.rs crates/tuner/src/main.rs
git commit -m "feat(tuner): meter styles + gradient cents color

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: `TunerConfig` — pure & tested

**Files:**
- Create: `crates/tuner/src/config.rs`
- Modify: `crates/tuner/Cargo.toml` (add `serde`)
- Modify: `crates/tuner/src/main.rs` (add `mod config;`)

- [ ] **Step 1: Add the `serde` dependency**

In `crates/tuner/Cargo.toml`, under `[dependencies]`, add:

```toml
serde.workspace = true
```

- [ ] **Step 2: Write the config struct + tests**

Create `crates/tuner/src/config.rs`:

```rust
//! Persisted tuner settings (meter style + enabled), stored as JSON via `overlay`.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct TunerConfig {
    #[serde(default)]
    pub meter_style_idx: usize,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Default for TunerConfig {
    fn default() -> Self {
        TunerConfig {
            meter_style_idx: 0,
            enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_json() {
        let cfg = TunerConfig {
            meter_style_idx: 2,
            enabled: false,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: TunerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.meter_style_idx, 2);
        assert!(!loaded.enabled);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let cfg: TunerConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.meter_style_idx, 0);
        assert!(cfg.enabled);
    }
}
```

Note: these tests use `serde_json`. Add it as a dev-dependency so the lib doesn't depend on it. In `crates/tuner/Cargo.toml` add:

```toml
[dev-dependencies]
serde_json.workspace = true
```

- [ ] **Step 3: Register the module**

In `crates/tuner/src/main.rs`, add:

```rust
mod config;
```

- [ ] **Step 4: Run tests**

Run: `cargo +stable test -p tuner config 2>&1 | tail -10`
Expected: PASS — 2 `config::tests`.

- [ ] **Step 5: Commit**

```bash
git add crates/tuner/Cargo.toml crates/tuner/src/config.rs crates/tuner/src/main.rs
git commit -m "feat(tuner): persisted TunerConfig (meter style + enabled)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Canvas meter (Needle + CenterBar) + pill view + State wiring

Render the note name, cents, and a Canvas meter (Needle/CenterBar) on a translucent pill. State now carries `enabled`/`style`/`strobe_phase`, initialized from `TunerConfig`. No tray yet (style is whatever config holds; default Needle).

**Files:**
- Modify: `crates/tuner/Cargo.toml` (enable iced `canvas` feature)
- Modify: `crates/tuner/src/meter.rs` (add the `Meter` canvas `Program`)
- Modify: `crates/tuner/src/main.rs` (State, view)

- [ ] **Step 1: Enable the iced `canvas` feature**

In `crates/tuner/Cargo.toml`, change `iced.workspace = true` to:

```toml
iced = { workspace = true, features = ["canvas"] }
```

- [ ] **Step 2: Add the `Meter` canvas program (Needle + CenterBar)**

Append to `crates/tuner/src/meter.rs`:

```rust
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Point, Rectangle, Renderer, Size, Theme};

/// Canvas program drawing the meter for the current reading.
pub struct Meter {
    pub cents: f64,
    pub style: MeterStyle,
    pub phase: f32,
    pub color: Color,
}

impl<Message> canvas::Program<Message> for Meter {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        let mid_x = w / 2.0;
        let mid_y = h / 2.0;
        let half = (w / 2.0 - 6.0).max(1.0);
        let track = Color::from_rgba(1.0, 1.0, 1.0, 0.35);

        // Baseline + center reference (drawn for Needle/CenterBar).
        if self.style != MeterStyle::Strobe {
            frame.stroke(
                &Path::line(Point::new(6.0, mid_y), Point::new(w - 6.0, mid_y)),
                Stroke::default().with_color(track).with_width(2.0),
            );
            frame.stroke(
                &Path::line(Point::new(mid_x, 2.0), Point::new(mid_x, h - 2.0)),
                Stroke::default().with_color(track).with_width(2.0),
            );
        }

        let pos = (self.cents.clamp(-50.0, 50.0) / 50.0) as f32;
        match self.style {
            MeterStyle::Needle => {
                let x = mid_x + pos * half;
                frame.stroke(
                    &Path::line(Point::new(x, 2.0), Point::new(x, h - 2.0)),
                    Stroke::default().with_color(self.color).with_width(4.0),
                );
            }
            MeterStyle::CenterBar => {
                let x = mid_x + pos * half;
                let (left, right) = if x >= mid_x { (mid_x, x) } else { (x, mid_x) };
                frame.fill_rectangle(
                    Point::new(left, mid_y - 4.0),
                    Size::new((right - left).max(1.0), 8.0),
                    self.color,
                );
            }
            MeterStyle::Strobe => {
                // Implemented in the strobe task.
            }
        }

        vec![frame.into_geometry()]
    }
}
```

NOTE (runtime/API): this uses iced 0.14 canvas APIs (`Frame::new`, `Path::line`, `Stroke::default().with_color().with_width()`, `frame.fill_rectangle`, `frame.into_geometry()`). If any method name differs in the resolved iced 0.14.x, adapt to the equivalent — confirm with `cargo +stable build -p tuner` and the docs for the exact patch version.

- [ ] **Step 3: Expand `State` and rewrite `view`**

In `crates/tuner/src/main.rs`:

1. Update the imports line `use iced::widget::{column, container, progress_bar, text};` to drop `progress_bar` and add `canvas`:

```rust
use iced::widget::{canvas, column, container, text};
```

2. Replace `#[derive(Default)] struct State { note: Option<Note> }` with:

```rust
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
```

3. Replace the whole `fn view(&self) -> Element<'_, Message>` body with:

```rust
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
```

- [ ] **Step 4: Build, clippy, and runtime-verify the meter**

Run: `cargo +stable build -p tuner 2>&1 | tail -5 && cargo +stable clippy -p tuner --all-targets -- -D warnings 2>&1 | tail -3 && cargo +stable test -p tuner 2>&1 | grep -E "test result:"`
Expected: build OK; clippy clean; tests `0 failed`.

Runtime (real COSMIC session): `cargo +stable build --release -p tuner && SCENO_DEBUG=1 ./target/release/tuner` — hum a note; the pill shows the note name, a centered **needle** meter, and the cents/`IN TUNE` label, color-graded. Kill with `pkill -f target/release/tuner`. (The owner does this; if launched from a headless shell it may not render.)

- [ ] **Step 5: Commit**

```bash
git add crates/tuner/Cargo.toml crates/tuner/src/meter.rs crates/tuner/src/main.rs
git commit -m "feat(tuner): Canvas needle/bar meter on a translucent pill

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Tray to toggle the overlay and switch meter style (persisted)

Add a ksni tray (mirroring lyrics' pattern). New messages `SetEnabled`/`SetMeterStyle` update state and persist a `TunerConfig`. The subscription spawns the tray + the audio thread into one message stream.

**Files:**
- Modify: `crates/tuner/Cargo.toml` (add `ksni`)
- Create: `crates/tuner/src/tray.rs`
- Modify: `crates/tuner/src/main.rs` (Message variants, update arms, subscription)

- [ ] **Step 1: Add the `ksni` dependency**

In `crates/tuner/Cargo.toml` `[dependencies]` add:

```toml
ksni.workspace = true
```

- [ ] **Step 2: Add the tray**

Create `crates/tuner/src/tray.rs`:

```rust
//! System-tray menu for the tuner: toggle the overlay and pick the meter style.

use futures::channel::mpsc::UnboundedSender;

use crate::Message;
use crate::meter::MeterStyle;

pub struct TunerTray {
    pub tx: UnboundedSender<Message>,
    pub enabled: bool,
    pub style: MeterStyle,
}

impl ksni::Tray for TunerTray {
    fn icon_name(&self) -> String {
        "audio-input-microphone".into()
    }
    fn title(&self) -> String {
        "sceno · tuner".into()
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let style = self.style;
        vec![
            CheckmarkItem {
                label: "Overlay ativo".into(),
                checked: self.enabled,
                activate: Box::new(|this: &mut Self| {
                    this.enabled = !this.enabled;
                    let _ = this.tx.unbounded_send(Message::SetEnabled(this.enabled));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: "Medidor".into(),
                submenu: vec![
                    RadioGroup {
                        selected: style.index(),
                        select: Box::new(|this: &mut Self, idx| {
                            this.style = MeterStyle::from_idx(idx);
                            let _ = this.tx.unbounded_send(Message::SetMeterStyle(this.style));
                        }),
                        options: vec![
                            RadioItem {
                                label: MeterStyle::Needle.label().into(),
                                ..Default::default()
                            },
                            RadioItem {
                                label: MeterStyle::CenterBar.label().into(),
                                ..Default::default()
                            },
                            RadioItem {
                                label: MeterStyle::Strobe.label().into(),
                                ..Default::default()
                            },
                        ],
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Sair".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}
```

- [ ] **Step 3: Add messages, update arms, and the combined subscription**

In `crates/tuner/src/main.rs`:

1. Add `mod tray;` to the module list.

2. Extend the message enum (it is annotated with `#[to_layer_message]`):

```rust
#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    PitchUpdate(Option<Note>),
    SetEnabled(bool),
    SetMeterStyle(meter::MeterStyle),
}
```

3. Replace the `update` body to handle the new variants and persist:

```rust
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PitchUpdate(n) => self.note = n,
            Message::SetEnabled(on) => {
                self.enabled = on;
                self.persist();
            }
            Message::SetMeterStyle(s) => {
                self.style = s;
                self.persist();
            }
            _ => {}
        }
        Task::none()
    }
```

4. Add a `persist` helper in `impl State` (add an `impl State { … }` block if none exists, above the `OverlayApp` impl):

```rust
impl State {
    fn persist(&self) {
        overlay::save(
            "tuner",
            &config::TunerConfig {
                meter_style_idx: self.style.index(),
                enabled: self.enabled,
            },
        );
    }
}
```

5. Replace `audio_stream` with a combined `event_stream` that spawns the tray + audio thread, and point the subscription at it. Replace the existing `fn audio_stream()` and the `subscription` body:

```rust
    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(event_stream)
    }
```

```rust
fn event_stream() -> BoxStream<'static, Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    let cfg: config::TunerConfig = overlay::load_config("tuner");

    ksni::TrayService::new(tray::TunerTray {
        tx: tx.clone(),
        enabled: cfg.enabled,
        style: meter::MeterStyle::from_idx(cfg.meter_style_idx),
    })
    .spawn();

    std::thread::spawn(move || audio::run(tx));

    Box::pin(rx)
}
```

(Keep the `use futures::channel::mpsc; use futures::stream::BoxStream;` imports already at the top.)

- [ ] **Step 4: Build, clippy, runtime-verify**

Run: `cargo +stable build -p tuner 2>&1 | tail -5 && cargo +stable clippy -p tuner --all-targets -- -D warnings 2>&1 | tail -3 && cargo +stable test -p tuner 2>&1 | grep -E "test result:"`
Expected: build OK; clippy clean; tests `0 failed`.

Runtime (owner): launch the release build; the tray shows **sceno · tuner** with "Overlay ativo" + "Medidor" (Agulha/Barra/Strobe) + "Sair". Toggling hides/shows the overlay; switching to Barra changes the meter; the choice survives a restart (persisted to `~/.config/sceno/tuner/config.json`).

- [ ] **Step 5: Commit**

```bash
git add crates/tuner/Cargo.toml crates/tuner/src/tray.rs crates/tuner/src/main.rs
git commit -m "feat(tuner): tray to toggle overlay + switch/persist meter style

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 8: Strobe style + gated animation tick

**Files:**
- Modify: `crates/tuner/src/meter.rs` (Strobe draw arm)
- Modify: `crates/tuner/src/main.rs` (`StrobeTick` message, phase advance, animated subscription)

- [ ] **Step 1: Draw the strobe bands**

In `crates/tuner/src/meter.rs`, replace the `MeterStyle::Strobe => { … }` arm in `Program::draw` with:

```rust
            MeterStyle::Strobe => {
                // Repeating vertical bands scrolled by `phase`; they appear to
                // freeze near 0¢ because `phase` barely advances there (see update).
                let offset = self.phase.rem_euclid(STROBE_BAND);
                let mut x = -STROBE_BAND + offset;
                while x < w {
                    frame.fill_rectangle(
                        Point::new(x, 2.0),
                        Size::new(STROBE_BAND / 2.0, h - 4.0),
                        self.color,
                    );
                    x += STROBE_BAND;
                }
            }
```

- [ ] **Step 2: Add the `StrobeTick` message + phase advance + animated subscription**

In `crates/tuner/src/main.rs`:

1. Add `StrobeTick` to the `Message` enum:

```rust
    StrobeTick,
```

2. Add a match arm in `update` (before the `_ => {}`):

```rust
            Message::StrobeTick => {
                if let Some(n) = &self.note {
                    let speed = (n.cents.clamp(-50.0, 50.0) / 50.0) as f32; // -1.0..1.0
                    self.strobe_phase =
                        (self.strobe_phase + speed * 6.0).rem_euclid(meter::STROBE_BAND);
                }
            }
```

3. Replace the `subscription` body to add the animation timer only while strobe is active:

```rust
    fn subscription(&self) -> Subscription<Message> {
        let events = Subscription::run(event_stream);
        if self.enabled && self.style == meter::MeterStyle::Strobe {
            let ticks = iced::time::every(std::time::Duration::from_millis(33))
                .map(|_| Message::StrobeTick);
            Subscription::batch([events, ticks])
        } else {
            events
        }
    }
```

NOTE (runtime/API): `iced::time::every` requires an iced async-runtime feature. If the build reports `time` is not found, add `tokio` to the iced features in `crates/tuner/Cargo.toml` (`features = ["canvas", "tokio"]`) and rebuild; verify the strobe animates without conflicting with the layer-shell executor. If a runtime conflict appears, fall back to `iced::window::frames().map(|_| Message::StrobeTick)` (continuous, still gated by this `if`).

- [ ] **Step 3: Build, clippy, runtime-verify**

Run: `cargo +stable build -p tuner 2>&1 | tail -5 && cargo +stable clippy -p tuner --all-targets -- -D warnings 2>&1 | tail -3 && cargo +stable test -p tuner 2>&1 | grep -E "test result:"`
Expected: build OK; clippy clean; tests `0 failed`.

Runtime (owner): switch the tray meter to **Strobe**; bands scroll while off-pitch and slow/freeze as you reach the note. Off-strobe styles add no timer.

- [ ] **Step 4: Commit**

```bash
git add crates/tuner/src/meter.rs crates/tuner/src/main.rs
git commit -m "feat(tuner): strobe meter with gated animation tick

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 9: Workspace-wide verification

**Files:** none (verification only).

- [ ] **Step 1: Full checks on the 1.96 toolchain**

Run:

```bash
cargo +stable fmt --all --check; echo "fmt-exit=$?"
cargo +stable clippy --all-targets -- -D warnings 2>&1 | tail -3
cargo +stable test --workspace 2>&1 | grep -E "test result:"
cargo +stable build --release 2>&1 | tail -3
ls target/release/lyrics target/release/tuner
```

Expected: `fmt-exit=0`; clippy no warnings; all `test result:` lines `0 failed` (overlay 7, lyrics 54, tuner 18 = 7 prior [`meter_position` test dropped with the unused fn in the canvas rewrite] + 4 smooth + 5 meter + 2 config); both release binaries present.

If `cargo +stable fmt --all --check` reports diffs, run `cargo +stable fmt --all`, re-verify, and commit as `style: rustfmt`.

- [ ] **Step 2: Owner runtime acceptance (manual)**

In a real COSMIC session: launch `./target/release/tuner`, confirm — needle meter by default on a translucent pill; smoothing makes the reading steady (vs. the old jitter); tray switches Agulha/Barra/Strobe and the choice persists across restarts; "Overlay ativo" hides/shows it.

---

## Notes

- Verify everything with `cargo +stable …` (1.96) — CI denies warnings on 1.96 and the local default may lag.
- Canvas + `iced::time` are the two API-risk spots (Tasks 6 and 8); each has a NOTE with the fallback.
- Pitch detection stays on `pitch-detection` (McLeod/MPM). pYIN/`loqa-voice-dsp` remain a documented future option (see spec) if voice robustness needs work.
