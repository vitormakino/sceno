# vocalize — call-and-response ear/voice trainer

Status: design
Date: 2026-06-24

## What it is

A 5th app binary in the sceno workspace. The app **plays + shows** a target note (or
chord); the user **sings** it; the app only advances once the user hits it. It is a
pitch-matching / ear-training warm-up — "ouça o tom e reproduza a afinação" — not sheet
reading.

Reuses existing infrastructure:

- **`pitch`** — `run_capture` (own cpal input stream, like `tuner`/`karaoke`), the
  smoothed frequency it hands consumers, `frequency_to_note` / `Note` / `midi_name`,
  `nearest_target`, and `cents_color` for live feedback.
- **`beat/click.rs` pattern** — sample-accurate sine rendering on a cpal **output** stream
  with an envelope. `vocalize` adapts this into an on-demand tone player (not clock-driven).
- **`overlay`** — `OverlayApp` trait, config I/O, single-instance, tray.

Name: `vocalize` (English single word, consistent with `lyrics`/`tuner`/`karaoke`/
`metronome`). The activity is closest to *solfejo*, but the app's core is pitch reproduction
from a heard reference, so the broader term fits.

## Decisions (from brainstorming)

- **Location:** new app crate in the sceno workspace.
- **Chord mode:** the voice is monophonic, so a "chord" is **collected one note at a time, in
  any order**. Each note of the displayed chord lights up when sung; all lit → advance.
- **Emission:** **play the sine tone + show the note name.** Tone replayable from the tray.
- **Octave:** **octave-folded** — a note counts if sung in any octave (men/women differ). The
  played reference tone uses a comfortable mid register.
- **Hit criteria:** sung pitch within a **cents window** held for a **sustain time**; both
  **adjustable from the tray** (sensible defaults below).
- **Exercise selection:** **random within a scale** (root + scale kind chosen in the tray),
  drawing single notes or diatonic chords depending on mode.
- **Note names:** shown in **both** formats — solfège + letter, e.g. `Mi (E)`.

## Architecture

Four units, each with one clear purpose.

### 1. `exercise.rs` — pure logic (no audio, no UI)

The testable core. No I/O.

- `Scale { root: i64 /* midi pitch class 0–11 */, kind: ScaleKind }` where
  `ScaleKind ∈ { Major, NaturalMinor, Chromatic }` (start small; more kinds are additive).
- `ScaleKind::degrees() -> &[i64]` semitone offsets from the root.
- Item generation: `next_item(scale, mode, prev) -> Item` where
  `Item = Vec<Note>` (one element for `Mode::Notes`, three for `Mode::Chords` — a diatonic
  triad built on a random scale degree). Avoids repeating the immediately previous item.
- **Matcher:** given the live octave-folded sung pitch class + cents offset, the cents window,
  and per-target sustain progress, decide which targets of the current item are now
  *collected*. Octave-folding = compare `midi mod 12`. A target is collected once the sung
  pitch class is within `±cents_window` continuously for `sustain_ms`.
  - `MatchState` tracks, per still-uncollected target, how long it has been held in-window
    (accumulated time, reset to 0 on a frame that falls out of window or goes silent).
  - `update(&mut self, sung: Option<(pitch_class, cents)>, dt_ms) -> Vec<usize>` returns
    indices newly collected this frame.

Unit tests: degree math, diatonic triad construction, octave-folding equality, sustain
accumulation + reset, "all collected" detection.

### 2. `tone.rs` — on-demand tone player (cpal output)

A cpal output stream that renders queued tones with an attack/decay envelope, adapted from
`beat::click`'s per-stream sine voice. Differences from `beat`: not clock-driven — it plays a
queued sequence of `(freq, duration)` segments then goes silent. A chord plays as a short
**arpejo** (the triad notes in sequence), so the user hears each pitch to reproduce.

- `Tone::play(&self, freqs: &[f64])` — queue one tone per freq (single note = one segment;
  chord = short ascending arpejo) at a comfortable register.
- Gated on an `audible` flag (the **Som** toggle).
- The reference register: map each item `Note` into a fixed comfortable octave for playback
  (e.g. octave 4) regardless of the octave-folded matching.

### 3. capture — `pitch::run_capture`

Own input stream, same as `tuner`/`karaoke`. The sink delivers the smoothed frequency; the UI
folds it to a pitch class + cents for the matcher and the live indicator.

### 4. `main.rs` + UI — `OverlayApp`

A tall fixed panel like `karaoke`: override `surface_height()` (≈220) and `stacks() = false`
so it owns its geometry instead of joining the thin-strip stacking.

Display:
- The current target large: chord notes as **chips** (`Mi (E)`), each chip turning **green**
  when collected; uncollected chips dim.
- A live indicator for the sung pitch coloured by `pitch::cents_color` (green ≤5¢ → amber →
  red), plus the sung note name, so the user sees how close they are.
- On "all collected": a brief success flash, then the next item.

## State machine

```
Present  — play reference tone (if audible) + show target; matcher DISABLED
   │  (tone finished)
   ▼
Listen   — live pitch → matcher.update(); collect targets; chips light up
   │  (all targets collected)
   ▼
Success  — brief flash
   │
   ▼  pick next item from the scale → Present
```

**Anti-cheat / mic bleed:** while the reference tone is playing through the speakers, the mic
would pick it up and auto-pass. The matcher is therefore **disabled during `Present`** (while
the tone sounds) and only runs in `Listen`, after the tone stops. The tone is short (~1 s for
a single note; the arpejo a bit longer).

The tone/visual flash and capture are driven the same way the other apps drive background
work: capture via the app's own `run_capture` thread; UI animation (success flash, live
indicator refresh) via the gated thread+mpsc `BoxStream` tick pattern used by `tuner`'s
Strobe / `lyrics`' `TimelineTick` — **not** `iced::time::every`, which conflicts with the
layer-shell executor.

## Config + tray

Persisted as `VocalizeConfig` in `~/.config/sceno/vocalize/config.json`:

```rust
struct VocalizeConfig {
    enabled: bool,        // master on/off
    audible: bool,        // play the reference tone
    scale_root: i64,      // midi pitch class 0–11
    scale_kind_idx: usize,// Major | NaturalMinor | Chromatic
    mode_idx: usize,      // Notes | Chords
    cents_window: f64,    // default 50.0
    sustain_ms: u64,      // default 500
}
```

Defaults: enabled, audible, C Major, Notes, ±50¢, 500 ms.

Tray (ksni, Portuguese labels like the other apps):
- **Tonalidade** — root (Dó…Si) + escala (Maior/Menor/Cromática) submenus.
- **Modo** — Notas / Acordes.
- **Tolerância** — cents window steps (e.g. ±25 / ±50 / ±75).
- **Sustentação** — sustain time steps (e.g. 300 / 500 / 800 ms).
- **Som** — toggle the reference tone.
- **Repetir tom** — replay the current item's tone.
- master enable/disable.

`#[to_layer_message]` means the `update` match needs a `_ => {}` arm (workspace convention).

## CLAUDE.md / README

Add a `vocalize` bullet to the app list in both `CLAUDE.md` (the "What this is" section) and
`README.md`, mirroring the `karaoke`/`metronome` entries (tall fixed panel; own data/config;
reuses `pitch` + the `beat` tone pattern).

## Testing

- Pure unit tests in `exercise.rs` (scale degrees, triads, octave-fold match, sustain
  accumulation/reset, all-collected).
- `tone.rs` / capture / UI are thin glue verified by running the app; no audio-device tests in
  CI.
- CI gate (workspace convention): `cargo +stable fmt --all --check`,
  `cargo +stable clippy --all-targets -- -D warnings`, `cargo +stable test --workspace`.

## Out of scope (YAGNI)

- Scoring/streaks/history, MIDI input, sheet-music rendering, microtonal scales beyond the
  three kinds, simultaneous polyphonic chord detection (impossible from one voice anyway),
  song/MPRIS integration. All can be added later if wanted.
```