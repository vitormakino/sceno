# sceno

A small family of minimal Wayland layer-shell overlay apps that show
glanceable information at the edge of your screen. They share a common base
crate (`overlay`) and are deliberately *not* a plugin system or one monolithic
app — each app is its own small binary.

## Apps

- **`lyrics`** — synced lyrics for whatever is playing. Reads now-playing from
  any MPRIS player (browsers, Spotify, mpv, …) over D-Bus and fetches
  time-synced lyrics from [LRCLIB](https://lrclib.net), anchored to the
  player's reported position so lines stay in time.
- **`tuner`** — a vocal tuner. Captures the microphone, detects pitch (McLeod
  method), and shows the nearest note plus a tuning meter.

## Building

Requires a Rust toolchain (edition 2024, rustc 1.85+) and these system
development packages on Linux:

```sh
# Debian/Ubuntu
sudo apt-get install libdbus-1-dev libasound2-dev libwayland-dev libxkbcommon-dev
```

- `libdbus-1-dev` — MPRIS (lyrics)
- `libasound2-dev` — ALSA capture via cpal (tuner)
- `libwayland-dev`, `libxkbcommon-dev` — the Wayland layer-shell stack

```sh
cargo build --release
./target/release/lyrics   # or
./target/release/tuner
```

The overlays are Wayland layer-shell surfaces and require a Wayland compositor
that supports the `wlr-layer-shell` protocol (e.g. COSMIC, Sway, Hyprland).

## Configuration

Per-app settings and cache follow the XDG base directories:

- config: `$XDG_CONFIG_HOME/sceno/<app>/config.json` (default `~/.config/sceno/<app>/`)
- cache: `$XDG_CACHE_HOME/sceno/<app>/` (default `~/.cache/sceno/<app>/`)

## License

MIT — see [LICENSE](LICENSE).
