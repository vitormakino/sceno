# CLAUDE.md

Guidance for working in the **sceno** repo. See `README.md` for user-facing docs and
`docs/specs/` + `docs/plans/` for design history.

## What this is

A Cargo workspace (`members = ["crates/*"]`, edition 2024) of minimal Wayland
**layer-shell** overlay apps. Each app is its own small binary; they are *not* a plugin
system. Four shared library crates + five app binaries:

- **`overlay`** (lib) — the shared shell. The `OverlayApp` trait (`namespace`, `update`,
  `view`, `subscription`, + default-implemented surface geometry: `surface_height`, `anchor`,
  `events_transparent`, `stacks`, `initial_margin`) + `overlay::run::<A>()` wires an app into
  `iced_layershell`. Also home to generic config I/O (`load_config::<T>`/`save`), XDG paths
  (`config_dir`/`cache_dir`/`data_dir`), `ensure_single_instance`, event-driven auto-stacking,
  and `SCENO_DEBUG` tracing (`overlay::debug(tag, args)`).
- **`pitch`** (lib) — mic capture + pitch math: `note` (`frequency_to_note`,
  `note_to_frequency`, `is_in_tune`, `Note` with a `midi` field), `smooth` (`Smoother`),
  `detect_frequency` (McLeod/MPM), `capture::run_capture` (cpal stream + 50 ms loop, calls a
  `sink(Option<Note>) -> bool` that returns `false` to stop), and `cents_color` (→ `[f32;3]`).
  Used by `tuner`, `karaoke`, and `vocalize`.
- **`media`** (lib) — now-playing + lyrics sources: `player` (MPRIS loop delivering a neutral
  `PlayerEvent` to a `sink(PlayerEvent) -> bool`), `sync::TimelineSync`, `cue` (`CueEntry` +
  `cue_at`), `lrc`/`lrclib` (LRCLIB fetch + on-disk store), `ultrastar` (`.txt`
  parser), and `library` (scan a folder, match by normalized artist/title). Used by `lyrics`,
  `karaoke`, and `metronome`.
- **`beat`** (lib) — metronome timing + signal: `clock` (`SharedClock`, an atomic-backed
  tempo + downbeat-phase anchor shared across threads; `beat_position_at`, `rephase`,
  `anchor_to`, `tap_bpm`), `click` (`run_click` — a cpal **output** stream that renders
  sample-accurate accented clicks off the shared clock, gated on `running && audible`), and
  `detect` (`run_detect` — best-effort tempo estimation from the system-audio **monitor** via
  energy-flux onsets + autocorrelation; pure-Rust, no aubio/C deps). Used by `metronome`.
- **`lyrics`** (bin) — synced caption overlay via `media` (MPRIS + LRCLIB). Owns its `SavedConfig`.
  Announces the now-playing title (`♪ Artist — Title`) during the first `ANNOUNCE_SECS` of a
  track while no lyric line is active. A per-song sync nudge corrects drift against arbitrary
  recordings: `SavedConfig.offsets` maps `TrackQuery::key()` → ms, added to the playback position
  (positive advances the lyrics, negative delays); the tray's **Sincronia** submenu nudges by
  ±100 ms or clears the current song's entry (a `0` offset removes the entry), and an active
  offset shows as a dim `⏱ ±NNN ms` chip in the overlay.
- **`tuner`** (bin) — vocal tuner via `pitch`: mic → note + tuning meter.
- **`karaoke`** (bin) — UltraStar karaoke: matches the playing track to a local `.txt` in its
  library and renders a scrolling Canvas pitch-lane (`lane.rs`), plus a live mic cursor
  (own `pitch::run_capture` stream) colored green when the sung pitch matches the target note
  (octave-folded). A tall, fixed panel: overrides `surface_height()=220` and `stacks()=false`
  so it owns its geometry instead of joining the thin-strip stacking. UltraStar `#GAP`/`#BPM`
  are calibrated to a specific recording, so a `KaraokeConfig.offset_ms` tray nudge corrects
  drift against arbitrary playback. `#RELATIVE` files are unsupported.
- **`metronome`** (bin) — beat overlay via `beat` (+ `media` for song sync). One `SharedClock`
  is the single source of truth read by both the audio click thread and the visual flash, so
  they never drift; it lives in a process-global `OnceLock` so the click thread (spawned in the
  subscription's `event_stream`) and the UI `State` share it. Three tempo `Source`s (tray
  **Fonte** submenu): **Manual** (tray ± / **Tap tempo**, which sets `manual_bpm` and anchors
  the tapped beat), **Song** (locks `#BPM`/`#GAP` of a matched UltraStar `.txt` to
  `TimelineSync` position, with a per-song `offsets` nudge like `lyrics`), and **Detect**
  (`beat::run_detect`, a *gated subscription* `detect_stream` that runs only while Detect is
  selected — dropping the subscription drops the sink so the capture thread exits). **Som** and
  **Flash** toggle the audible click and the visual `Beats` dot-row (`meter.rs`) independently.
- **`vocalize`** (bin) — call-and-response ear/voice trainer via `pitch`: plays + shows a
  target note or chord and only advances once the user sings it. Pure exercise logic
  (`exercise.rs`: `Scale`/`ScaleKind`/`Mode`, `PlayStyle`, `item_at`, the octave-folded
  `Matcher`, `note_label`) is unit-tested. `Mode` picks the shape — Notas / Power chord
  (root + an *absolute* perfect fifth, `[at(d), at(d)+7]`, so it stays perfect on every
  degree) / Tríade (`d,d+2,d+4`) / Tétrade (diatonic 7th `d,d+2,d+4,d+6`); a chord is
  collected one note at a time, in any order. `tone.rs` renders the reference tone on demand
  off a cpal **output** stream (the `beat::click` sine pattern, queued not clock-driven); its
  `ToneSynth` sums N simultaneous voices of one of two timbres — an FM **electric piano**
  (`Timbre::ElectricPiano`, default: carrier + a fast-decaying modulator + a percussive
  amplitude envelope) or a pure **`Sine`** — selected by the tray **Timbre** submenu. `PlayStyle`
  (tray **Reprodução**) plays a chord either **Junto** (one block segment, default) or **Arpejo**
  (one segment per note).
  A tall fixed panel (`surface_height()=160`, `stacks()=false`) shows the target as
  solfège+letter chips (`Dó (C)`) that turn green as each note is collected; the matcher is
  gated off while the tone sounds so mic bleed can't auto-pass. A 33 ms gated tick drives
  sustain timing + the success flash. `VocalizeConfig { enabled, audible, scale_root,
  scale_kind_idx, mode_idx, play_style_idx, timbre_idx, cents_window, sustain_ms }` persists the tray
  choices (Tonalidade, Escala, Modo, Reprodução, Timbre, Tolerância, Sustentação, Som, Repetir tom).

Each app owns a per-app data folder `~/.local/share/sceno/<app>` (`overlay::data_dir("<app>")`),
so file kinds don't intermingle: `karaoke` and `metronome` read UltraStar `.txt` from
`…/sceno/karaoke` and `…/sceno/metronome` respectively, while
`lyrics` persists LRCLIB hits as `Artist - Title.lrc` into `…/sceno/lyrics` — that single
human-named file is both the re-download guard and a browsable copy (there is no separate hash
cache under `~/.cache`).

Stack: `iced` 0.14 under `iced_layershell` 0.18 (wgpu), `ksni` tray, `serde`,
`pitch-detection` (McLeod/MPM), `cpal`, `mpris`, `ureq`. `#[to_layer_message]` injects extra
`Message` variants, so `update` match blocks need a `_ => {}` arm.

## Platforms

Linux/Wayland is the primary target. `tuner` and `vocalize` also build on **macOS**:
`overlay::run` has two backends behind `#[cfg(target_os = "linux")]` — the layer-shell
shell on Linux, a plain `iced` always-on-top transparent window (centered, pinned near the
bottom via `window::Position::SpecificWith`) elsewhere. The layer-shell–only pieces are
Linux-gated: `iced_layershell`/`dbus` deps, `OverlayApp::{margin_changed, anchor}`, the
`#[to_layer_message]` attribute (apply it via `#[cfg_attr(target_os = "linux", …)]`), the
auto-stacking in `stack.rs`, and the `OverlayMessage` `TryInto<…>` bound. Apps return
`overlay::Result` (a platform alias) from `main`. The system tray (`ksni`) is Linux-only;
on macOS the overlay runs from the persisted config with **no tray and no click-through**
(`iced` 0.14 has no `cursor_hittest`) — both are documented follow-ups. `lyrics`/`karaoke`/
`metronome` stay Linux-only (they need the `mpris` now-playing source). CI verifies the
macOS subset via the `check-macos` job (`overlay`, `pitch`, `tuner`, `vocalize`). When
editing a `tuner`/`vocalize` `update` match, a `_ => {}` arm that becomes unreachable off
Linux must be `#[cfg(target_os = "linux")]`-gated (unless a guarded arm keeps it reachable).
See `docs/plans/2026-06-26-macos-compat.md`.

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

Color feedback (`pitch::cents_color`, shared with karaoke): green `[0.30,0.90,0.30]` within
±5¢ → amber `[0.95,0.75,0.20]` by ±25¢ → red `[0.90,0.25,0.25]` by ±50¢ (symmetric, clamped).

Anti-jitter smoothing (`crates/tuner/src/smooth.rs`, `Smoother`): EMA on frequency
(`ALPHA = 0.25`) + a "hold" of `HOLD_FRAMES = 6` `None` frames (~300 ms) before dropping,
then resets cold so the next onset isn't dragged toward the old pitch.

**Reference pitch + instrument presets:** `pitch::run_capture` hands consumers the
smoothed *frequency* (Hz); the tuner maps it to a note with its chosen `a4_hz` reference
(tray **Referência**: 432/440/442/443) and `Instrument` preset (tray **Instrumento**:
Cromático/Violão/Baixo/Ukulele/Violino, `crates/tuner/src/instrument.rs`). A non-chromatic
preset snaps the readout to the nearest open string via `pitch::nearest_target` →
`Note::at_midi`; chromatic uses `frequency_to_note`. `State.last_freq` is kept so a
reference/instrument change re-maps immediately (`State::remap`). `karaoke` maps the same
frequency with the `pitch::A4` constant (unchanged).

Style/on-off/reference/instrument are persisted as
`TunerConfig { meter_style_idx, enabled, a4_hz, instrument_idx }` in
`~/.config/sceno/tuner/config.json` (Needle + enabled + 440 Hz + Chromatic by default).

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
