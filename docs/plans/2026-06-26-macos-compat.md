# macOS compatibility — phase 1 (`tuner` + `vocalize`)

*2026-06-26*

## Why

`sceno` was Linux/Wayland-only. The platform coupling lived in three places:

- **Window shell** — `iced_layershell` (Wayland *layer-shell*): transparent,
  click-through, bottom-anchored, auto-stacked surfaces.
- **Tray** — `ksni` (StatusNotifierItem over D-Bus).
- **Now-playing** — `mpris` (D-Bus), used by `lyrics` / `karaoke` / `metronome`.

Everything else is portable: `cpal` audio (mic + tone), the `pitch` math, and all
`iced` `view`/`update`/`subscription` logic.

This phase ports the two apps that need **no** now-playing — `tuner` and
`vocalize` (mic + overlay + tray only) — as the lowest-risk first step, and adds
a macOS CI job so the non-Linux code paths are compile-verified.

## What changed

### `overlay` — the shared seam
- `OverlayApp` stays the single trait apps implement. The layer-shell–flavored
  bits are now `#[cfg(target_os = "linux")]`: `margin_changed`, `anchor`, and the
  `TryInto<LayerShellCustomActionWithId>` bound (extracted into an `OverlayMessage`
  trait-alias that drops that bound off Linux).
- `run::<A>()` has two backends:
  - **Linux** — unchanged: `iced_layershell` + slot auto-stacking + D-Bus reflow.
  - **off Linux** — plain `iced::application(...)` with a borderless, transparent,
    `Level::AlwaysOnTop` window, centered horizontally and pinned near the bottom
    edge via `window::Position::SpecificWith` (mirrors the layer-shell strip).
- `overlay::Result` is a platform alias (`iced_layershell::Result` on Linux,
  `iced::Result` elsewhere); apps return it from `main`.
- `stack.rs`: the geometry math (`Margin`, `margin_for_slot`, the size/gap
  constants) stays cross-platform; the `flock` slot ownership + D-Bus reflow are
  Linux-gated.
- `Cargo.toml`: `iced_layershell` + `dbus` moved under
  `[target.'cfg(target_os = "linux")'.dependencies]`.

### `tuner` / `vocalize`
- `#[to_layer_message]` → `#[cfg_attr(target_os = "linux", iced_layershell::to_layer_message)]`.
- `mod tray;` and the `ksni` spawn are Linux-gated; `iced_layershell` + `ksni` are
  Linux-only deps.
- `main() -> overlay::Result`.
- The `update` catch-all that absorbs the macro-injected variants is Linux-gated in
  `tuner` (its match is otherwise exhaustive off Linux); `vocalize` keeps it
  (its guarded `Replay` arm still needs a fallback).

### CI
- New `check-macos` job (`macos-latest`) builds + clippies (`-D warnings`) + tests
  `overlay`, `pitch`, `tuner`, `vocalize`. The Linux job still covers the whole
  workspace.

## Known limitation
`iced` 0.14 `window::Settings` exposes no `cursor_hittest`, so the macOS overlay is
**not click-through** yet — the window floats on top but captures clicks. On Linux
the layer-shell `events_transparent` path is unchanged. A real fix
(`ignoresMouseEvents` via `objc2` / raw-window-handle, or upstream `iced` support)
is a follow-up.

There is **no system tray on macOS** in this phase: `tray-icon`/Cocoa menus must be
created and pumped on the main thread, which `iced` already owns, with no hook for
us to share it. So the macOS overlay runs from the persisted config with no menu.
Bringing the tray to macOS is a follow-up (likely needs deeper event-loop
integration).

## Out of scope (follow-ups)
- Cross-platform now-playing (`media::player`: AppleScript on macOS, GSMTC on
  Windows) → port `lyrics` / `karaoke` / `metronome`.
- macOS click-through and system tray.
- Windows.
