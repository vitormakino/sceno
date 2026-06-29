# `lyrics` on macOS — now-playing via Apple Music / Spotify (AppleScript)

*2026-06-29*

## Why

macOS-compat phase 1 (`2026-06-26-macos-compat.md`) ported `tuner` and `vocalize` (mic +
overlay only) and left as an explicit follow-up:

> "Cross-platform now-playing (`media::player`: AppleScript on macOS …) → port `lyrics` /
> `karaoke` / `metronome`."

`lyrics` was Linux-only for a single reason: its now-playing source (`media::player`) used
**MPRIS/D-Bus**, which has no macOS equivalent. This change adds a macOS now-playing backend so
`lyrics` — the most broadly useful app — runs on the Mac, doubling the family's macOS footprint.

## Decision: AppleScript, not the private MediaRemote framework

There is **no public macOS API to read another app's now-playing metadata** (MPRIS is a Linux
*read* protocol with no macOS twin). The only system-wide reader is the **private**
`MediaRemote.framework`, and **Apple locked it down in macOS 15.4 (March 2025)** — `mediaremoted`
now serves only `com.apple.*` clients; third-party processes get nil. It remains locked through
macOS 15.x and macOS 26.

So we use the **per-app AppleScript** path (Music.app + Spotify.app via `osascript`):

- **Immune to the 15.4 lockdown** — the single biggest risk to any MediaRemote approach; keeps
  working on macOS 26+.
- **Maps 1:1 onto the existing `PlayerEvent`** — title/artist/album/duration/**position**/state
  is exactly what `media::player` already carries; position drives LRC sync.
- **Zero new dependencies** — pure `std::process::Command` over `osascript`, fitting the repo's
  minimal-deps ethos. (The `media-remote` crate via a Perl adapter is the *fallback* if we ever
  want system-wide/browser coverage + artwork — it's a private-framework hack with higher
  maintenance risk; not adopted now.)
- Same architecture as the leading macOS lyrics apps (LyricFever, LyricGlow), which both fell
  back to per-app AppleScript after the 15.4 break.

**Lyrics still come from LRCLIB.** Apple Music's own synced lyrics are not exposed by any public
or private API (licensed, streamed server-side). Only the *metadata source* changes; the whole
`media::lrclib`/`media::lrc` fetch path is reused verbatim, identical to Linux.

## What changed

### `media` — backend split behind the existing seam
- `crates/media/Cargo.toml`: `mpris` moved under `[target.'cfg(target_os = "linux")'.dependencies]`.
- `crates/media/src/player.rs`: the track-change + LRCLIB fetch-retry + event-emission logic
  (previously inline in the MPRIS loop) was extracted into a shared `Tracker` driven by a
  platform-neutral `Snapshot { query, sync }`. `PlayerEvent`, `build_query` + the metadata-cleaning
  helpers, and `fetch_cues` stay shared.
  - **Linux backend** (`#[cfg(target_os = "linux")]`): the MPRIS loop, now building a `Snapshot`
    and calling `Tracker::step`. Behavior unchanged.
  - **macOS backend** (`#[cfg(not(target_os = "linux"))]`): polls Music then Spotify each
    `POLL_INTERVAL` via `osascript`; the script is **guarded by `is running`** so it never
    auto-launches a player. `run_osascript` reads only stdout (empty on any failure — including a
    not-installed app whose `tell` block fails to compile, or a TCC denial — so everything
    degrades to "nothing playing", never a crash). `parse_now_playing` is a **pure, unit-tested**
    function that splits the pipe-delimited line and normalizes **Spotify's millisecond duration**
    to seconds (Music reports seconds). `is_browser` is Linux-gated (native players have
    trustworthy tags, so `build_query(..., from_browser = false)`).

### `lyrics` — macOS-compat treatment (same playbook as tuner/vocalize)
- `crates/lyrics/Cargo.toml`: `iced_layershell` + `ksni` moved to Linux-only deps.
- `#[to_layer_message]` → `#[cfg_attr(target_os = "linux", …)]`; tray (`LyricsTray` + impls) and
  the `ksni` spawn Linux-gated; `margin_changed` Linux-gated; `main() -> overlay::Result`.
  The crate-level `#![cfg_attr(not(target_os = "linux"), allow(dead_code))]` covers the tray-only
  helpers (`NUDGE_MS`, `nudge`) off Linux.
- **Live config-watch added to `lyrics`** (`ReloadConfig` + `watch_config_stream`, comparing via a
  new `current_config()` so a self-write doesn't bounce). This **revises** the earlier "apps with a
  tray skip the watcher" decision: macOS `lyrics` has no tray, so the JSON file is its only config
  surface. Enabled on all platforms (inert on Linux). `SavedConfig` gained `Clone, PartialEq`.

### Build wiring
- `.cargo/config.toml` `mac`/`mac-clippy`/`mac-test` aliases and the CI `check-macos` job both add
  `-p media -p lyrics`.

## macOS permissions (TCC Automation)
The first `osascript` Apple Event triggers a one-time Automation prompt per (host → target). From
a terminal the grant attaches to the terminal host; rebuilding the binary can re-trigger it —
fine for local/dev. Denial returns error `-1743`; we treat any failure as "no player" (backoff),
never crash. Distribution as a signed `.app` with `NSAppleEventsUsageDescription` is a follow-up,
alongside the existing click-through/tray macOS follow-ups.

## Verification
- **Unit (CI both jobs):** `parse_now_playing` — Music seconds, Spotify ms→s, paused/playing,
  empty `""` → `None`, short line → `None`, title-with-spaces, missing-artist fallthrough; plus
  the guard assertion on `now_playing_script`. Existing metadata-cleaning tests unchanged.
- **macOS (manual):** `cargo mac{,-clippy -- -D warnings,-test}` green; `./target/debug/lyrics`
  with Apple Music then Spotify playing → synced caption + intro announcement; pause freezes the
  line; closing both players clears the overlay without crashing; editing
  `~/.config/sceno/lyrics/config.json` applies live.
- **Linux (CI `check`):** whole-workspace build/clippy/test stay green; the MPRIS path is
  behaviorally unchanged (the `Tracker` extraction is pure reorganization).

## Out of scope (follow-ups)
- `MediaRemote` backend (system-wide coverage incl. browsers + artwork).
- Port of `karaoke`/`metronome` to macOS (they'd reuse this backend).
- Signed `.app` / Info.plist; macOS click-through and tray; Windows (GSMTC).
