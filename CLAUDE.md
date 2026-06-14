# CLAUDE.md

Guidance for working in the **sceno** repo. See `README.md` for user-facing docs and
`docs/specs/` + `docs/plans/` for design history.

## What this is

A Cargo workspace (`members = ["crates/*"]`, edition 2024) of minimal Wayland
**layer-shell** overlay apps. Each app is its own small binary; they are *not* a plugin
system. Shared base crate + two apps:

- **`overlay`** — the shared shell. The `OverlayApp` trait (`namespace`, `update`, `view`,
  `subscription`) + `overlay::run::<A>()` wires an app into `iced_layershell`. Also home to
  generic config I/O (`load_config::<T>`/`save`), XDG paths (`config_dir`/`cache_dir`),
  `ensure_single_instance`, and `SCENO_DEBUG` tracing (`overlay::debug(tag, args)`).
- **`lyrics`** — synced lyrics via MPRIS (D-Bus) + LRCLIB. Owns its `SavedConfig`.
- **`tuner`** — vocal tuner: mic (cpal/ALSA) → pitch detection → note + tuning meter.

Stack: `iced` 0.14 under `iced_layershell` 0.18 (wgpu), `ksni` tray, `serde`,
`pitch-detection` (McLeod/MPM), `cpal`. `#[to_layer_message]` injects extra `Message`
variants, so `update` match blocks need a `_ => {}` arm.

## Tuner meter styles

The tuner display is a translucent dark "pill" showing the note name + signed cents, with a
`Canvas`-drawn tuning meter. Three styles live in `crates/tuner/src/meter.rs`
(`MeterStyle`), selectable from the tray's **Medidor** submenu and persisted:

- **`Needle`** (default, label "Agulha") — a needle at `center + (clamp(cents,±50)/50)·half`
  over a baseline with a 0¢ center reference.
- **`CenterBar`** (label "Barra") — a bar growing from center toward the deviation side.
- **`Strobe`** (label "Strobe") — scrolling vertical bands (`STROBE_BAND = 24px`); scroll
  speed/direction ∝ cents so the pattern appears to freeze near 0¢. Animated by a gated
  33 ms tick (`StrobeTick`) added to `subscription()` **only** while `enabled && style ==
  Strobe`; the tick is a thread+mpsc `BoxStream` (the lyrics `TimelineTick` pattern), *not*
  `iced::time::every` (which conflicts with the layer-shell executor).

Color feedback (`meter::cents_color`): green `[0.30,0.90,0.30]` within ±5¢ → amber
`[0.95,0.75,0.20]` by ±25¢ → red `[0.90,0.25,0.25]` by ±50¢ (symmetric, clamped).

Anti-jitter smoothing (`crates/tuner/src/smooth.rs`, `Smoother`): EMA on frequency
(`ALPHA = 0.25`) + a "hold" of `HOLD_FRAMES = 6` `None` frames (~300 ms) before dropping,
then resets cold so the next onset isn't dragged toward the old pitch.

Style + on/off are persisted as `TunerConfig { meter_style_idx, enabled }` in
`~/.config/sceno/tuner/config.json` (Needle + enabled by default).

## Conventions

- **Config/cache** follow XDG: `~/.config/sceno/<app>/config.json`,
  `~/.cache/sceno/<app>/`. Each app owns its config struct; `overlay::{load_config, save}`
  are generic over any serde type.
- **Tracing:** `SCENO_DEBUG=1` enables stderr traces via `overlay::debug("<tag>", args)`.
- **Pitch detection** stays on `pitch-detection` (pure Rust, no C deps). pYIN/microdsp are
  documented future options in the spec, not adopted.

## Verifying changes

CI runs **clippy with `-D warnings` on Rust 1.96 stable** — newer lints than older local
toolchains catch. Always verify with the `+stable` (1.96) toolchain before committing:

```sh
cargo +stable fmt --all --check
cargo +stable clippy --all-targets -- -D warnings
cargo +stable test --workspace
```

Note: clippy `--all-targets` flags `dead_code` on the **bin** target for code only used by
later commits/tests; when splitting work across commits, the symbol becomes "used" once its
consumer lands — don't paper over it with `#[allow(dead_code)]`.

System dev packages (Linux): `libdbus-1-dev libasound2-dev libwayland-dev libxkbcommon-dev`.
