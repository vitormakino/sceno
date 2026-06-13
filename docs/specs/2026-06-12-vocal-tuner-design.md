# Design: Vocal Tuner app + overlay workspace extraction

Date: 2026-06-12

## Context

The lyrics overlay (MPRIS + LRCLIB → Wayland layer-shell) is working. The owner
wants the project to grow into a small family of on-screen overlay apps that share
a common base, **without** becoming one monolithic "monster" app. Rather than a
plugin system, the chosen shape is a **Cargo workspace**: one shared `overlay`
library plus several small, single-purpose binaries.

To avoid premature abstraction, the base is being extracted now that there is a
**real second consumer**: a **vocal tuner** that shows, on the same overlay, the
musical note currently being sung into the microphone. Lyrics (network/MPRIS) and
the tuner (audio DSP) share *nothing* in their data sources — only the on-screen
overlay — which is exactly what justifies a shared rendering library and separate
apps.

## Goals

- Extract the Wayland overlay plumbing into a reusable `overlay` library crate.
- Refactor the existing lyrics app onto that library (no behavior change).
- Build a new `tuner` app: mic → pitch → note + tuning meter on the overlay.
- Keep each binary small and single-responsibility.

## Non-goals

- No dynamic/WASM plugin system (deferred; may add external-process widgets later).
- No multi-widget composition in one process (each app is its own overlay surface).
- The tuner does not know the song's target melody; it only shows what is sung.

## Architecture

Cargo workspace (repo name still TBD — tracked as a deferred item):

```
<repo>/
├── Cargo.toml                     # [workspace]
└── crates/
    ├── overlay/   (lib)  layer-shell surface, render runner, position, tray, config
    ├── lyrics/    (bin)  MPRIS + LRCLIB → overlay        # current app, refactored
    └── tuner/     (bin)  microphone → pitch → overlay    # new
```

### `overlay` library (extracted from current `src/main.rs`)

Owns the standardized plumbing so each app only writes its own logic:

- Layer-shell surface setup (anchor, margin, size, transparent, click-through).
- A **runner** that wraps `iced_layershell::application` and injects: the shared
  style (transparent bg, white text), the tray (ksni) with the common items
  (enabled toggle, position Top/Bottom, font size, quit), and config
  load/save (font size, position, enabled).
- Each app supplies its own `State` / `update` / `view` (iced); the runner wires
  the rest. Lyrics' `view` renders a caption; the tuner's `view` renders note +
  meter. The shared `Position` / `FontSize` / config types live here.

Interface sketch (refined during implementation):

```rust
// overlay::run(app) where the app provides state/update/view + a title,
// and overlay supplies layer-shell, tray, config, position handling.
```

### `tuner` app

```
mic ──cpal──▶ ring buffer ──~2048-sample window──▶ pitch-detection (McLeod/YIN)
                                                        │ freq + clarity
                                          clarity < threshold → "no note" (hidden)
                                                        │
                          freq → note+octave+cents   (midi = 69 + 12·log2(f/440))
                                                        │ smoothing (EMA/median) to de-jitter
                                                        ▼
                                  view: note name + horizontal tuning meter
```

- **Capture:** `cpal` opens the default input device; its audio-thread callback
  pushes f32 samples into a lock-free ring buffer.
- **Detection:** an analysis tick (~50 ms) reads the latest window and runs the
  McLeod Pitch Method via the `pitch-detection` crate → fundamental frequency +
  clarity.
- **Gate:** below a clarity/amplitude threshold → emit "no pitch" → overlay hidden.
- **Mapping:** `freq → (note, octave, cents)`; cents = `100 * (midi - round(midi))`.
- **Smoothing:** EMA/median over the last few estimates + hysteresis on the note
  name so it does not flicker between adjacent notes.
- **Meter:** a slim horizontal bar (flat ◄─►　sharp) with a marker positioned by
  cents in [-50, +50]; turns green when |cents| < ~5 (in tune).

Chosen display (approved):

```
          C#4
   ♭ ───────●──────── ♯
              +12¢  (sharp)
```

## Defaults (approved)

- Reference pitch **A = 440 Hz**; note names use **sharps** (C#, not Db).
- Display is **hidden** on silence / unclear pitch (no "—" placeholder).
- Tuned for **voice** (~70–1200 Hz) but works for instruments too.

## Dependencies (new, in `tuner`)

- `cpal` — cross-platform audio input (PipeWire/ALSA on Linux).
- `pitch-detection` — McLeod/YIN fundamental-frequency estimation.
- a lock-free ring buffer (`ringbuf` or equivalent) for the audio→analysis handoff.

## Testing

- **Pure / deterministic:** `freq → note/octave/cents` (440→A4 0¢, 445→A4 ≈+20¢,
  466.16→A#4 ≈0¢), `cents → meter position`, note-name table, smoothing math.
- **Detector:** synthesize a sine wave of a known frequency in-memory, run the
  detector, assert detected freq ≈ input (no hardware).
- **Audio capture (cpal):** manual test only.
- The existing lyrics unit tests must still pass after the refactor onto `overlay`.

## Verification

- `cargo test` across the workspace; `cargo clippy` clean.
- Lyrics app behaves identically after the refactor (run, play a track, see synced
  lyrics).
- Tuner: sing/hum into the mic, confirm the note + meter track pitch and the meter
  reads in-tune (green, centered) on a steady held note; goes blank on silence.

## Deferred (tracked, not in this spec)

- General repo/workspace name.
- Publishing the lyrics app (identity scrub to GitHub noreply; professionalization
  pack: CI, Cargo metadata, LICENSE/README, the `call_with_retry` 4xx fix).
