# Design: `tuner` pro — adjustable reference pitch + instrument presets

**Date:** 2026-06-18
**Status:** spec (brainstorming → spec)

## Context

The `tuner` shows the nearest note + a tuning meter for whatever is sung/played.
It is fixed to **A = 440 Hz** and **chromatic** (nearest of all 12 notes). The two
features every dedicated tuner (GuitarTuna, Boss Tuner, Fender Tune, Pano Tuner)
ships — and that turn the app from "singer's note readout" into "instrumentalist's
tuner" — are:

1. **Adjustable reference pitch** (A=432 for some ensembles, A=442/443 for orchestras).
2. **Instrument presets / alternate tunings** — pick *Guitar*, and the tuner snaps to
   the six target strings and tells you which string you're closest to and how far off,
   instead of guessing chromatic notes near the open-string pitch.

The pitch math already supports an arbitrary reference: `frequency_to_note(freq, a4)`
takes `a4` as a parameter (`crates/pitch/src/note.rs`). The only reason the tuner is
locked to 440 is that `pitch::run_capture` hardcodes `frequency_to_note(f, A4)` deep in
the capture loop (`crates/pitch/src/capture.rs:131`). Freeing the reference is mostly
"stop hardcoding the constant".

## Goals

- A persisted, tray-selectable **reference pitch** that retunes the readout live.
- A persisted, tray-selectable **instrument preset**: chromatic (default) + common
  string sets. With a preset active, show the **nearest target string** and the cents
  to *that* string.
- Both features reuse the existing meter/color feedback unchanged (cents still drive it).

## Non-goals

- No polyphonic / chord detection (still one fundamental at a time).
- No transposition/capo (tracked out-of-scope; small follow-on).
- No custom user-defined tunings UI (presets are a fixed built-in list this pass).
- No change to the meter styles, smoothing, or capture DSP.

## Decisions (locked)

| Decision | Choice |
|----------|--------|
| Where the reference is applied | In the **app**, not the capture thread |
| Capture output | `run_capture` emits the smoothed **frequency** (`Option<f64>`), not a `Note` |
| Reference values | Tray radio: **432 / 440 / 442 / 443 Hz** (440 default); persisted as `a4_hz: f64` |
| Instrument presets | Chromatic (default), Guitar (E2 A2 D3 G3 B3 E4), Bass (B0? no — E1 A1 D2 G2), Ukulele (G4 C4 E4 A4), Violin (G3 D4 A4 E5) |
| Preset string match | Auto: nearest target by frequency ratio to the chosen `a4` |
| Display with a preset | Note name = the **target string** (e.g. `A2`), cents = deviation from it |
| Display chromatic | Unchanged (nearest of all 12 notes) |
| Config | `TunerConfig { meter_style_idx, enabled, a4_hz, instrument_idx }` |

### Why move note mapping out of the capture thread

`run_capture` currently smooths the **frequency** (EMA, hold frames) and only then maps
to a `Note` with the hardcoded `A4`. Smoothing is reference-independent, so the natural
seam is: capture emits the smoothed `Option<f64>` Hz, and each consumer maps with the
reference (and, for the tuner, the instrument) *it* wants. This makes a live reference
change a pure app-state update (no thread restart), and gives the instrument-preset
logic the raw frequency it needs to pick the nearest string.

**Cross-crate impact:** `karaoke` also calls `pitch::run_capture`. It keeps current
behavior by mapping the emitted frequency with the existing `A4` constant
(`frequency_to_note(f, pitch::A4)`) — a one-line change, no behavior difference.

## Pitch-layer changes (`crates/pitch`)

- `capture.rs`: change the sink to `FnMut(Option<f64>) -> bool` and emit the smoothed
  frequency directly (drop the in-loop `frequency_to_note(f, A4)` and the `Note`/`A4`
  imports there).
- Add a small helper for instrument tuning in `note.rs` (kept here so the note math
  lives in one crate):

```rust
/// A target string: a fixed MIDI note the instrument tunes to.
pub struct Target { pub name: &'static str, pub octave: i32, pub midi: i64 }

/// The target nearest to `freq` (Hz) under reference `a4`, with signed cents.
/// `targets` is the instrument's open-string MIDI set. Returns None if empty.
pub fn nearest_target(freq: f64, a4: f64, targets: &[i64]) -> Option<(i64, f64)>;
```

`nearest_target` computes the fractional MIDI of `freq` under `a4` (same formula as
`frequency_to_note`), picks the closest target MIDI, and returns `(midi, cents)` where
`cents = 100 * (frac_midi - target_midi)`, clamped to the meter's domain by the caller.

Unit-test: E2 string (≈82.41 Hz at A440) → returns E2's midi, ~0¢; a pitch a bit sharp
of A2 → A2 with positive cents; reference at 432 shifts the matched cents accordingly.

## App changes (`crates/tuner`)

### Instrument enum (mirrors `MeterStyle`)

`instrument.rs`: an `Instrument` enum with `index`/`from_idx`/`label`, plus
`targets(self) -> &'static [i64]` (the open-string MIDI numbers; empty for Chromatic).
Labels in PT to match the tray (`Cromático`, `Violão`, `Baixo`, `Ukulele`, `Violino`).

### Config

`config.rs`: add `a4_hz: f64` (default 440.0) and `instrument_idx: usize` (default 0)
to `TunerConfig`, each `#[serde(default = ...)]` so existing configs load. Extend the
roundtrip + missing-fields tests.

### State / messages / mapping

`main.rs`:
- `State` gains `a4_hz: f64` and `instrument: Instrument`; `PitchUpdate` now carries
  `Option<f64>` (frequency). On update, map to the displayed readout:
  - Chromatic → `frequency_to_note(freq, self.a4_hz)` (name/octave/cents as today).
  - Preset → `nearest_target(freq, self.a4_hz, instrument.targets())` → build the
    name/octave from the matched target and the returned cents.
  - Store the result as the existing `Option<Note>` so `view` is unchanged. (Add a
    `Note` constructor from a target+cents, or reuse `frequency_to_note` then override
    name/octave/cents — chosen: a small `Note::from_target(midi, cents)` helper.)
- New messages `SetReference(f64)` and `SetInstrument(Instrument)`, each persisting.
- `persist` writes the two new fields; `event_stream` passes them to the tray.

### Tray (`tray.rs`)

Two new submenus after **Medidor**:
- **Referência** — `RadioGroup` over 432/440/442/443 (selected by matching `a4_hz`).
- **Instrumento** — `RadioGroup` over the `Instrument` variants.

`TunerTray` gains `a4_hz` and `instrument` fields for the selected-state.

### Display nuance

With a preset active and the input far from any string (e.g. nearest target is >±50¢),
the meter still pegs at its clamp; that is the expected "very out of tune" reading, no
special-casing. The note name shows the target being tuned *toward*, which is what a
guitarist expects when tuning a slack string up.

## Testing

Pure/deterministic (no hardware):

- `pitch::nearest_target`: nearest selection, signed cents sign, empty targets → None,
  reference shift (432 vs 440) changes cents for the same input Hz.
- `Instrument`: `from_idx`/`index` round-trip; `targets` lengths (Guitar=6, Bass=4,
  Ukulele=4, Violin=4, Chromatic=0).
- `TunerConfig`: roundtrip with the new fields; `{}` → defaults (440.0, 0).
- App mapping: a frequency at A440 with Chromatic → `A4 0¢`; same frequency with
  reference 432 → slightly sharp; with Guitar preset, 110 Hz → `A2 ~0¢`.
- `karaoke`: existing tests stay green after the `run_capture` signature change (its
  mapping moves to `frequency_to_note(f, pitch::A4)`).

## Verification

- `cargo +stable fmt --all --check`, `clippy --all-targets -- -D warnings`,
  `test --workspace` green.
- Manual: hum/play; switch **Referência** to 442 and watch a held A read slightly flat;
  switch **Instrumento** to *Violão* and pluck the low E — the readout locks to `E2`
  and the meter centers when in tune. Switch back to *Cromático* for free-note display.

## Out of scope (tracked, separate items)

- Transposition / capo offset.
- Custom user-defined tunings + a "lock to a single string" mode.
- Polyphonic detection.
- Reference fine-nudge (±1 Hz) if 432/440/442/443 proves too coarse.
