# Design: tuner meters, tray, and pitch smoothing

**Date:** 2026-06-13
**Status:** approved (brainstorming → spec)

## Context

The `tuner` app works (mic → `pitch-detection` McLeod → note + cents), verified on real
hardware. But the display is rudimentary and was called out as ugly: it uses a stock
`progress_bar` as the tuning meter, which fills left→right (reads as "% full") instead of the
universal **centered** tuner metaphor (center = in tune, left = flat, right = sharp). It also has
no center reference, only binary green/white color, no flat/sharp cue, and the raw cents value
jitters badly (±50¢ frame-to-frame, visible in `SCENO_DEBUG` traces).

This redesign makes the tuner look and feel like a real tuner, grounded in established tuner
UI/UX: a centered meter with a 0¢ reference, gradient color feedback, direction cues, and
smoothing. It adds **three selectable meter styles** switchable from a system tray (the tuner has
no tray today), with the choice persisted. The tuner becoming a second tray consumer also lets us
generalize config I/O so persistence isn't duplicated.

## Decisions (locked via brainstorming)

| Decision | Choice |
|----------|--------|
| Meter styles | Three: **Needle** (default), **CenterBar**, **Strobe** |
| Style switching | Tray submenu; choice persisted |
| Build scope | All three at once, including the animated Strobe |
| Background | Translucent dark "pill" behind the content (for contrast over any wallpaper) |
| Tray items | **Overlay ativo** (show/hide) · **Medidor** submenu (radio) · **Sair** |
| Smoothing | Hand-rolled EMA on frequency + short note "hold" (no new dependency) |
| Pitch library | **Keep `pitch-detection`** (McLeod/MPM, pure Rust). Not reinventing the wheel. |
| Color | Gradient green (≤±5¢) → amber (~±25¢) → red (≥±50¢) |

## Signal-processing library survey (June 2026)

The "signal processing" in scope is (a) pitch detection and (b) smoothing. We are not hand-writing
DSP — pitch detection already comes from a library.

- **`pitch-detection`** (current): pure-Rust McLeod/MPM + YIN. Mature, no C deps, already working
  and unit-tested here. **Kept.**
- **`aubio-rs`**: bindings to the C `aubio` lib (richer: onset/tempo/MIDI). Adds a C build/CI
  dependency — rejected for a monophonic tuner that doesn't need the extra features.
- **`microdsp`**: pure-Rust MPM with downsampling + overlapping windows, `no_std`. Slightly more
  capable than `pitch-detection`, but switching is re-plumbing for marginal gain — not now.
- **`pyin-rs` / `loqa-voice-dsp`**: probabilistic pYIN, voice-optimized, returns a voiced
  probability (a cleaner silence gate than clarity+hold). Genuinely better for breathy/noisy
  **voice** — recorded as a **future option** if voice robustness becomes a pain point.
- **Smoothing**: EMA is a single multiply-add; a dependency would be over-engineering. Hand-rolled.

## Components

### 1. Pitch smoothing (`crates/tuner/src/audio.rs`)

The analysis loop already computes a raw frequency each ~50 ms. Add a small stateful smoother used
inside the loop, kept pure/testable:

- **EMA on frequency:** `smoothed = α·new + (1−α)·prev`, `α ≈ 0.25` (const, tunable). Convert the
  smoothed frequency to a `Note` (existing `frequency_to_note`).
- **Hold on dropout:** when detection returns `None`, keep emitting the last note for up to
  `HOLD ≈ 300 ms` (a small counter of consecutive `None` analysis frames), then emit `None`. The
  EMA resets after a real silence (hold expiry) so the next onset isn't dragged toward the old
  pitch.
- Extract the decision into a testable unit, e.g. a `Smoother` struct with
  `fn update(&mut self, raw: Option<f64>) -> Option<f64>` (EMA + hold), independent of cpal. Tests
  feed sequences and assert EMA convergence and hold/expiry behavior.

The loop emits `Message::PitchUpdate(Option<Note>)` exactly as today; only the values are smoothed.

### 2. Meter rendering (`crates/tuner/src/meter.rs`, new)

Use the `iced` **Canvas** widget for custom 2D drawing (the stock `progress_bar` is removed).

- `pub enum MeterStyle { Needle, CenterBar, Strobe }` with `index()`, `from_idx(usize)`, and a
  human label (`"Agulha"`, `"Barra"`, `"Strobe"`).
- `fn cents_color(cents: f64) -> iced::Color`: green `(0.3,0.9,0.3)` within ±5¢; linearly
  interpolate green→amber `(0.95,0.75,0.2)` over 5..25¢; amber→red `(0.9,0.25,0.25)` over 25..50¢;
  clamp beyond. Pure, unit-tested at the boundaries.
- A canvas `Program` that, given `(note, cents, style, strobe_phase)`, draws:
  - **Needle (default):** a horizontal baseline across the width; a filled triangle / tick at the
    0¢ center; minor ticks at ±10/20/30/40/50¢; a needle (filled triangle or thick line) at
    `x = center + (clamp(cents,−50,50)/50)·half_width`, drawn in `cents_color`. `♭` label far left,
    `♯` far right.
  - **CenterBar:** a thin full-width baseline with a center mark; a filled bar growing from center
    toward the deviation side, width `∝ |cents|`, colored by `cents_color`.
  - **Strobe:** repeating diagonal/vertical bands scrolled by `strobe_phase`; scroll speed and
    direction `∝ cents` (flat scrolls one way, sharp the other), approaching zero near 0¢ so the
    pattern "freezes"; tinted by `cents_color`. `strobe_phase` is supplied by the animation tick
    (§5).
- **Layout & pill:** `view()` composes a `container` with a **rounded, semi-transparent dark
  background** (e.g. `Color::from_rgba(0,0,0,0.45)`, radius ~12) holding a centered `column!`:
  the note name + octave as large `text` in `cents_color`; the signed cents as smaller `text`
  (e.g. `"+18¢"` / `"IN TUNE"` within deadband); and the `Canvas` meter (fixed height ~24 px)
  below. When `enabled == false` or `note == None`, render an empty widget (nothing visible).

### 3. Tray (`crates/tuner/src/tray.rs`, new)

A `ksni::Tray` impl mirroring the lyrics pattern (the menu is app-specific, not shared):

- `icon_name`: a generic audio icon; `title`: `"sceno · tuner"`.
- Menu: `CheckmarkItem` **Overlay ativo** (toggles, sends `Message::SetEnabled`); `Separator`;
  `SubMenu` **Medidor** with a `RadioGroup` (Agulha / Barra / Strobe → `Message::SetMeterStyle`);
  `Separator`; `StandardItem` **Sair** (`std::process::exit(0)`).
- A subscription bridge spawns the `ksni::TrayService` on a thread and forwards selections into the
  app via an `mpsc` channel (same shape as lyrics' `event_stream`). The tray's initial checkbox /
  radio reflect the loaded `TunerConfig`.

### 4. Config persistence — generic I/O (`crates/overlay/src/settings.rs`, `crates/tuner/src/config.rs`)

Generalize the overlay config I/O so persistence logic lives in one place:

- `pub fn load_config<T: Default + serde::de::DeserializeOwned>(app: &str) -> T` and
  `pub fn save<T: serde::Serialize>(app: &str, cfg: &T)` (keeps the `cfg!(test)` early-return in
  `save`; `config_path(app)` unchanged → `~/.config/sceno/<app>/config.json`).
- The lyrics-specific `SavedConfig` **moves into the `lyrics` crate**; lyrics calls
  `overlay::load_config::<SavedConfig>("lyrics")` / `overlay::save("lyrics", &cfg)`. `FontSize` and
  `Position` remain in `overlay` (reusable UI helpers).
- New `tuner` `TunerConfig { meter_style_idx: usize, enabled: bool }` (serde, `Default` = Needle +
  enabled), loaded at startup and saved on every tray change. Serde roundtrip unit-tested.

### 5. Strobe animation (`crates/tuner/src/main.rs`)

- `State` gains `strobe_phase: f32`. A new `Message::StrobeTick` advances it.
- `subscription()` returns `Subscription::batch` of the audio stream plus — **only when
  `style == Strobe && enabled`** — an `iced::time::every(~33 ms)` timer mapped to `StrobeTick`.
  Needle/CenterBar add no timer (they redraw on `PitchUpdate`).

### State & messages (`crates/tuner/src/main.rs`)

- `struct State { note: Option<Note>, enabled: bool, style: MeterStyle, strobe_phase: f32 }`,
  initialized from `TunerConfig` in `Default`/startup.
- `Message` (under `#[to_layer_message]`) gains `SetEnabled(bool)`, `SetMeterStyle(MeterStyle)`,
  `StrobeTick`, alongside existing `PitchUpdate(Option<Note>)`.
- `update`: `PitchUpdate` sets `note`; `SetEnabled` toggles + `overlay::save`; `SetMeterStyle` sets
  style + `overlay::save`; `StrobeTick` advances `strobe_phase` (wrapping).

## Testing

- **Smoother**: EMA convergence on a constant input; hold keeps the last value for N `None` frames
  then expires; EMA resets after expiry.
- **`cents_color`**: green within ±5; amber-ish near ±25; red by ±50; symmetric for ±.
- **`MeterStyle`**: `index`/`from_idx` roundtrip incl. out-of-range → default.
- **`TunerConfig`**: serde roundtrip; missing fields fall back to defaults.
- Canvas drawing and the tray are verified at runtime (overlay on the COSMIC session, `SCENO_DEBUG`
  for the pitch trace); not unit-tested (visual / system I/O).

## Risks

- **Canvas under `iced_layershell` (wgpu):** confirm the `canvas` widget renders correctly on the
  transparent layer-shell surface — validate early in implementation before building all three
  styles.
- **Strobe readability** in an 80 px strip: tune band size/speed during the runtime check; it is
  the least conventional style and may need iteration.

## Out of scope

- Switching pitch libraries (pYIN/microdsp) — documented as a future option.
- Configurable A4 reference (stays 440 Hz), transposition, or per-string guitar mode.
- Generalizing the *tray menu* itself (menus are app-specific; only config I/O is shared).
- Font-size control for the tuner (lyrics-only for now).
