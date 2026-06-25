# vocalize — electric-piano timbre (FM synthesis)

Status: design
Date: 2026-06-25

## What it is

`vocalize` currently plays its reference tone as a **pure sine** (summed per voice, with a
linear attack/release ramp and flat sustain) — thin and "beepy". This adds a richer,
**electric-piano** timbre via lightweight FM synthesis, selectable from the tray, defaulting
to the new sound. Pure Rust, no new dependencies, no shipped assets — consistent with the
workspace's "pure Rust, no C deps, minimal" ethos.

## Decisions (from brainstorming)

- **Approach:** pure-Rust **FM synthesis** (2-operator, Rhodes/EP-style) + a **percussive
  decay envelope**. Not a soundfont/sampled instrument (rejected: adds a dependency + a
  multi-MB asset, overkill for a ~1 s reference cue).
- **Selectable:** a tray **Timbre** submenu — `Piano elétrico` (default) / `Senóide` (the
  current sine, kept as a clean/simple option). Persisted.
- The matcher, the chord modes (`Mode`), and the play styles (`PlayStyle`) are unchanged.

## Synthesis design (in `tone.rs`)

A 2-operator FM voice per note (per frequency in a segment):

- Carrier oscillator at the fundamental `f`; modulator at `f * MOD_RATIO` (ratio ≈ 1:1 for a
  warm Rhodes-ish tone).
- Per sample: `s = sin(carrier_phase + index(t) * sin(mod_phase))`, where the **modulation
  index decays quickly** (`index(t) = EP_INDEX * exp(-t / EP_MOD_DECAY)`, `EP_MOD_DECAY ≈
  0.18 s`) to give the bright "tine" attack that mellows into the body.
- **Amplitude envelope is percussive**: `amp(t) = exp(-t / EP_AMP_DECAY)` (`EP_AMP_DECAY ≈
  0.9–1.2 s`) instead of a flat sustain, plus the existing short attack/release ramp
  (`RAMP_SECS`) so onsets/cutoffs don't click.
- `t` is seconds since the segment's strike (`pos = total - left`, `t = pos / sr`), so every
  voice in a "together" segment is struck at `t = 0` simultaneously; an arpejo strikes each
  note at its own segment origin (the existing per-segment timing already gives this).
- All FM constants (`MOD_RATIO`, `EP_INDEX`, `EP_MOD_DECAY`, `EP_AMP_DECAY`) live at the top
  of `tone.rs` and are easy to retune after listening.

The `Sine` timbre keeps today's behavior exactly: `sin(carrier_phase)` with the linear
attack/release ramp and flat middle.

### `Timbre` type

`enum Timbre { ElectricPiano, Sine }` lives **in `tone.rs`** (a synthesis concern; keeps the
module self-contained — `tone.rs` does not depend on `exercise`). Stable indices:
`ElectricPiano = 0`, `Sine = 1`. Provides `index()`, `from_idx(usize)`, `label()`
("Piano elétrico" / "Senóide"), `const ALL: [Timbre; 2]` — mirroring `Mode`/`PlayStyle`.
`config`/`main`/`tray` reference `tone::Timbre`.

### `ToneSynth` changes

- Add `mod_phases: Vec<f64>` (second phase per voice, for FM) alongside the existing
  `cur_freqs` + carrier `phases`, and a `timbre: Timbre` field for the active segment.
- On segment start: `phases` and `mod_phases` both reset to `vec![0.0; n]`.
- `next()` branches on `timbre`: `Sine` → current path; `ElectricPiano` → the FM voice with
  the percussive envelope above. Voices are summed and divided by `n` (existing anti-clip),
  and the result stays in `[-1, 1] * GAIN`.
- Borrow note: read each `cur_freqs[i]` into a local before mutating `phases[i]` /
  `mod_phases[i]` (same disjoint-access pattern the current single-voice code relies on).

### `Tone::play` / `Pending`

`Tone::play(&self, freqs, together, timbre: Timbre) -> Duration`. The shared `Pending` slot
becomes `Option<(Vec<f64>, bool, Timbre)>`; the audio callback hands the timbre to
`ToneSynth::load(&freqs, together, timbre)`. Durations are unchanged (`NOTE_SECS` for a single
note or a `together` chord; `ARP_SECS * len` for an arpejo) — the matcher's `present_until`
gate is unaffected.

## Wiring (mirrors the existing tray toggles)

- **`config.rs`:** add `timbre_idx: usize` (`#[serde(default)]`, default `0` = EP) to
  `VocalizeConfig` + `Default`; update the roundtrip/defaults tests.
- **`main.rs`:** `State` gains `timbre: Timbre` (from `cfg.timbre_idx`); `advance()`, the
  `Replay` arm, and `Default` pass it to `tone.play`; new `Message::SetTimbre(tone::Timbre)`
  with an `update` arm following set→persist→reset; `persist()` writes
  `timbre_idx: self.timbre.index()`.
- **`tray.rs`:** `VocalizeTray` gains `timbre`; insert a **"Timbre"** `SubMenu` (RadioGroup
  over `Timbre::ALL` → `Message::SetTimbre`) near the other audio controls (after "Modo"
  / "Reprodução"); `event_stream`'s constructor passes
  `timbre: Timbre::from_idx(cfg.timbre_idx)`.

## Testing

- **`tone.rs`** (no prior unit tests; the struct is private so same-file tests reach it):
  - `Timbre::from_idx`/`index` roundtrip (+ unknown idx → `ElectricPiano`).
  - **Bounded output:** for each timbre, a `ToneSynth` loaded with a chord produces only
    finite samples within `[-1, 1]` across the first ~`NOTE_SECS` of samples (guards against
    NaN / clipping / silence).
  - **EP decays:** peak |sample| over an early window (first ~50 ms) is meaningfully greater
    than over a late window (~700–750 ms) for `ElectricPiano` — verifies the percussive
    envelope. Use a peak-over-window (not a single instantaneous sample, which crosses zero).
- **`config.rs`:** roundtrip + defaults updated for `timbre_idx`.
- CI gate (workspace): `cargo +stable fmt --all --check`,
  `cargo +stable clippy --all-targets -- -D warnings`, `cargo +stable test --workspace`.
- **Manual (the real judge of timbre):** run `cargo run -p vocalize`; via the tray switch
  **Timbre** → Piano elétrico vs Senóide and confirm the EP sounds like a struck/decaying
  electric piano (and chords/arpejos still work); confirm the choice survives a restart.

## Backward compatibility

`timbre_idx` defaults to `0` (Electric piano), so existing configs and the default experience
now play the **EP timbre**; the old pure sine remains available as `Senóide`.

## Out of scope (YAGNI)

Adjustable brightness/index controls, multiple EP variants, soundfont/MIDI playback, velocity
sensitivity. The FM constants are tunable in source if the default EP needs adjustment.
