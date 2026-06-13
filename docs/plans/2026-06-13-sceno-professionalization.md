# sceno Professionalization Pack Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the workspace publishable as `sceno` — fix a retry bug, adopt XDG paths under a `sceno` basename, migrate to edition 2024, sweep rustfmt, and add LICENSE/README/Cargo metadata/CI — stopping before the actual GitHub publish.

**Architecture:** A sequence of six independent commits. Code/config first (Tasks 1–4), project files next (Task 5), CI last (Task 6). No git-history rewrite (the personal email is kept by choice). Pure logic is extracted so the retry policy and XDG path resolution are unit-testable without network or environment mutation.

**Tech Stack:** Rust (Cargo workspace, edition 2024), `ureq`, `serde`, `iced`/`iced_layershell`, `cpal`, `mpris`. GitHub Actions for CI.

Spec: `docs/specs/2026-06-13-sceno-professionalization-design.md`.

**Decisions (locked):** name `sceno`; license MIT; identity `Vitor Makino <vitor.makino@gmail.com>`; repository `https://github.com/vitormakino/sceno`; stop before creating the repo / pushing.

---

### Task 1: Fix `call_with_retry` to retry only transient failures

Today `call_with_retry` returns immediately on `404` but **retries every other error**, including non-transient `4xx` (e.g. `400`, `403`). Extract a pure `status_is_retryable(code)` predicate (the testable unit) and use it so only `5xx` and transport/network errors are retried.

**Files:**
- Modify: `crates/lyrics/src/lrclib.rs` (the `call_with_retry` fn at lines 116–129, and the `tests` module)

- [ ] **Step 1: Write the failing test**

Add this test inside the existing `#[cfg(test)] mod tests` in `crates/lyrics/src/lrclib.rs` (e.g. just after `cache_disabled_in_tests`):

```rust
    #[test]
    fn only_5xx_status_is_retryable() {
        // Client errors (4xx) are definitive — never retried.
        assert!(!status_is_retryable(400));
        assert!(!status_is_retryable(403));
        assert!(!status_is_retryable(404));
        assert!(!status_is_retryable(429));
        // Server errors (5xx) are transient — retry.
        assert!(status_is_retryable(500));
        assert!(status_is_retryable(503));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p lyrics only_5xx_status_is_retryable 2>&1 | tail -20`
Expected: FAIL — compile error `cannot find function status_is_retryable in this scope`.

- [ ] **Step 3: Add the predicate and use it in `call_with_retry`**

In `crates/lyrics/src/lrclib.rs`, replace the whole `call_with_retry` function (lines 116–129) with:

```rust
/// Whether an HTTP status code warrants a retry. Only server-side `5xx`
/// errors are transient; `4xx` (including `404`/`429`) are definitive.
fn status_is_retryable(code: u16) -> bool {
    (500..600).contains(&code)
}

/// Issue a request, retrying only transient failures (transport/network errors
/// and `5xx`) with a short backoff. Any `4xx` is a definitive result and
/// returns immediately. The request is rebuilt per attempt since
/// `ureq::Request` is single-use.
fn call_with_retry(build: impl Fn() -> ureq::Request) -> Option<String> {
    for attempt in 0..=HTTP_RETRIES {
        match build().call() {
            Ok(resp) => return resp.into_string().ok(),
            Err(ureq::Error::Status(code, _))
                if status_is_retryable(code) && attempt < HTTP_RETRIES =>
            {
                std::thread::sleep(RETRY_BACKOFF);
            }
            Err(ureq::Error::Transport(_)) if attempt < HTTP_RETRIES => {
                std::thread::sleep(RETRY_BACKOFF);
            }
            // 4xx, a non-retryable status, or retries exhausted.
            Err(_) => return None,
        }
    }
    None
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p lyrics 2>&1 | tail -15`
Expected: PASS — all lyrics tests, including `only_5xx_status_is_retryable`.

- [ ] **Step 5: Commit**

```bash
git add crates/lyrics/src/lrclib.rs
git commit -m "fix(lyrics): only retry transient LRCLIB failures (5xx + transport)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: XDG-compliant paths under a `sceno` basename

Move config/cache off the hardcoded `~/.config/lyrics-on-screen` & `~/.cache/lyrics-on-screen` onto XDG base dirs under a `sceno/<app>` scheme, via shared helpers in the `overlay` crate. Thread the app name through the config API so apps don't collide, rename the lyrics namespace to `lyrics`, and update the LRCLIB User-Agent to the new name/URL.

**Files:**
- Create: `crates/overlay/src/paths.rs`
- Modify: `crates/overlay/src/lib.rs:3-4` (module + re-exports)
- Modify: `crates/overlay/src/settings.rs:91-116` (`config_path`/`load_config`/`save` take `app`)
- Modify: `crates/lyrics/src/main.rs` (add `APP` const; namespace; 3 config call sites)
- Modify: `crates/lyrics/src/lrclib.rs:11-16,143-147` (User-Agent + delegate `cache_dir`)

- [ ] **Step 1: Create `crates/overlay/src/paths.rs` with the pure resolver + helpers + tests**

```rust
//! XDG base-directory resolution for sceno apps.
//!
//! Config lives under `$XDG_CONFIG_HOME/sceno/<app>` (fallback
//! `~/.config/sceno/<app>`); cache under `$XDG_CACHE_HOME/sceno/<app>`
//! (fallback `~/.cache/sceno/<app>`).

use std::path::PathBuf;

/// Pure XDG base resolution (env values injected for testability): use `xdg`
/// when it is `Some` and non-empty, otherwise `$HOME/<fallback>`; then append
/// `sceno/<app>`. Returns `None` only when neither source is available.
fn resolve(xdg: Option<&str>, home: Option<&str>, fallback: &str, app: &str) -> Option<PathBuf> {
    let root = match xdg {
        Some(x) if !x.is_empty() => PathBuf::from(x),
        _ => PathBuf::from(home?).join(fallback),
    };
    Some(root.join("sceno").join(app))
}

/// `$XDG_CONFIG_HOME/sceno/<app>` (fallback `~/.config/sceno/<app>`).
pub fn config_dir(app: &str) -> Option<PathBuf> {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let home = std::env::var("HOME").ok();
    resolve(xdg.as_deref(), home.as_deref(), ".config", app)
}

/// `$XDG_CACHE_HOME/sceno/<app>` (fallback `~/.cache/sceno/<app>`).
pub fn cache_dir(app: &str) -> Option<PathBuf> {
    let xdg = std::env::var("XDG_CACHE_HOME").ok();
    let home = std::env::var("HOME").ok();
    resolve(xdg.as_deref(), home.as_deref(), ".cache", app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_var_takes_precedence() {
        let p = resolve(Some("/run/cfg"), Some("/home/u"), ".config", "lyrics").unwrap();
        assert_eq!(p, PathBuf::from("/run/cfg/sceno/lyrics"));
    }

    #[test]
    fn empty_xdg_falls_back_to_home() {
        let p = resolve(Some(""), Some("/home/u"), ".config", "lyrics").unwrap();
        assert_eq!(p, PathBuf::from("/home/u/.config/sceno/lyrics"));
    }

    #[test]
    fn missing_xdg_uses_home() {
        let p = resolve(None, Some("/home/u"), ".cache", "tuner").unwrap();
        assert_eq!(p, PathBuf::from("/home/u/.cache/sceno/tuner"));
    }

    #[test]
    fn no_home_no_xdg_is_none() {
        assert!(resolve(None, None, ".config", "lyrics").is_none());
    }
}
```

- [ ] **Step 2: Register the module and re-export the helpers**

In `crates/overlay/src/lib.rs`, change lines 3–4 from:

```rust
mod settings;
pub use settings::{load_config, save, FontSize, Position, SavedConfig};
```

to:

```rust
mod paths;
mod settings;
pub use paths::{cache_dir, config_dir};
pub use settings::{load_config, save, FontSize, Position, SavedConfig};
```

- [ ] **Step 3: Run the new tests to verify they pass**

Run: `cargo test -p overlay 2>&1 | tail -15`
Expected: PASS — the 4 `paths::tests` plus the existing 5 settings tests.

- [ ] **Step 4: Make the config API take an `app` name**

In `crates/overlay/src/settings.rs`, replace `config_path`/`load_config`/`save` (lines 91–116) with:

```rust
pub fn config_path(app: &str) -> Option<std::path::PathBuf> {
    crate::paths::config_dir(app).map(|d| d.join("config.json"))
}

pub fn load_config(app: &str) -> SavedConfig {
    config_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &str, font_size: FontSize, enabled: bool) {
    if cfg!(test) {
        return;
    }
    let Some(path) = config_path(app) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cfg = SavedConfig { font_size_idx: font_size.index(), enabled };
    if let Ok(json) = serde_json::to_string(&cfg) {
        let _ = std::fs::write(path, json);
    }
}
```

- [ ] **Step 5: Update lyrics — add `APP` const, rename namespace, pass `app` at call sites**

In `crates/lyrics/src/main.rs`:

1. Near the top of the file (after the `use` imports, before `struct State`), add:

```rust
/// App name: used for the Wayland namespace, the single-instance lock, and the
/// config/cache directory (`~/.config/sceno/lyrics`, `~/.cache/sceno/lyrics`).
const APP: &str = "lyrics";
```

2. Change `namespace()` (lines 190–192) from `"lyrics-on-screen"` to use the const:

```rust
    fn namespace() -> &'static str {
        APP
    }
```

3. Line 71 — `let cfg = overlay::load_config();` → `let cfg = overlay::load_config(APP);`
4. Line 228 — `overlay::save(state.font_size, state.enabled);` → `overlay::save(APP, state.font_size, state.enabled);`
5. Line 232 — `overlay::save(state.font_size, state.enabled);` → `overlay::save(APP, state.font_size, state.enabled);`
6. Line 290 — `let cfg = overlay::load_config();` → `let cfg = overlay::load_config(APP);`

- [ ] **Step 6: Point the lyrics cache at the shared helper + update the User-Agent**

In `crates/lyrics/src/lrclib.rs`:

1. Replace the `USER_AGENT` const (lines 12–16) with:

```rust
const USER_AGENT: &str = concat!(
    "sceno-lyrics v",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/vitormakino/sceno)"
);
```

2. Replace the local `cache_dir` function (lines 143–147) with a delegation:

```rust
fn cache_dir() -> Option<PathBuf> {
    overlay::cache_dir("lyrics")
}
```

- [ ] **Step 7: Build and run the whole test suite**

Run: `cargo build --workspace 2>&1 | tail -5 && cargo test --workspace 2>&1 | grep -E "test result:|error\["`
Expected: build OK; every `test result:` line shows `0 failed` (overlay 9, lyrics 51, tuner 8).

- [ ] **Step 8: Commit**

```bash
git add crates/overlay/src/paths.rs crates/overlay/src/lib.rs crates/overlay/src/settings.rs crates/lyrics/src/main.rs crates/lyrics/src/lrclib.rs
git commit -m "feat: XDG-compliant config/cache paths under a sceno basename

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Migrate to edition 2024

Run the edition migration on the current (2021) sources, then bump the workspace edition.

**Files:**
- Modify: `Cargo.toml:6` (`workspace.package.edition`)
- Possibly modify: any `crates/*/src/*.rs` that `cargo fix` rewrites (expected: none or trivial)

- [ ] **Step 1: Apply edition migrations while still on 2021**

Run: `cargo fix --edition --workspace 2>&1 | tail -20`
Expected: completes without error. For this small codebase it likely reports no changes; if it rewrites anything, that's the migration.

- [ ] **Step 2: Bump the workspace edition**

In `Cargo.toml`, change line 6 from `edition = "2021"` to:

```toml
edition = "2024"
```

- [ ] **Step 3: Verify build, tests, and clippy are green on 2024**

Run: `cargo build --workspace 2>&1 | tail -5 && cargo test --workspace 2>&1 | grep -E "test result:|error\[" && cargo clippy --all-targets 2>&1 | tail -5`
Expected: build OK; all `test result:` lines `0 failed`; clippy prints `Finished` with no warnings.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: migrate workspace to Rust edition 2024

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: rustfmt sweep

One mechanical commit normalizing the whole workspace to rustfmt (the formatting debt deferred from the tuner branch). Done after Tasks 1–3 so it also formats their new code.

**Files:**
- Modify: any `.rs` files rustfmt reformats (workspace-wide)

- [ ] **Step 1: Format the workspace**

Run: `cargo fmt --all`
Expected: no output (success).

- [ ] **Step 2: Verify formatting is now clean and tests still pass**

Run: `cargo fmt --all --check; echo "fmt-exit=$?" && cargo test --workspace 2>&1 | grep -E "test result:|error\["`
Expected: `fmt-exit=0` (no diff); all `test result:` lines `0 failed`.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "style: rustfmt the entire workspace

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: LICENSE, README, and Cargo metadata

Add the MIT license, a workspace README, and publish-ready Cargo metadata.

**Files:**
- Create: `LICENSE`
- Create: `README.md`
- Modify: `Cargo.toml` (`[workspace.package]`)
- Modify: `crates/overlay/Cargo.toml`, `crates/lyrics/Cargo.toml`, `crates/tuner/Cargo.toml` (`[package]` metadata)

- [ ] **Step 1: Create `LICENSE` (MIT)**

```
MIT License

Copyright (c) 2026 Vitor Makino

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 2: Create `README.md`**

```markdown
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
```

- [ ] **Step 3: Add workspace-wide Cargo metadata**

In `Cargo.toml`, replace the `[workspace.package]` section (lines 5–7) with:

```toml
[workspace.package]
edition = "2024"
version = "0.1.0"
authors = ["Vitor Makino <vitor.makino@gmail.com>"]
license = "MIT"
repository = "https://github.com/vitormakino/sceno"
readme = "README.md"
keywords = ["wayland", "overlay", "layer-shell", "lyrics", "tuner"]
categories = ["gui", "multimedia"]
```

(Note: `edition = "2024"` was set in Task 3; keep it.)

- [ ] **Step 4: Add per-crate `[package]` metadata**

In `crates/overlay/Cargo.toml`, replace the `[package]` block with:

```toml
[package]
name = "overlay"
description = "Shared Wayland layer-shell overlay base for the sceno apps."
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
```

In `crates/lyrics/Cargo.toml`, replace the `[package]` block with:

```toml
[package]
name = "lyrics"
description = "Synced lyrics overlay driven by MPRIS + LRCLIB."
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
```

In `crates/tuner/Cargo.toml`, replace the `[package]` block with:

```toml
[package]
name = "tuner"
description = "Vocal tuner overlay: microphone pitch detection to note + tuning meter."
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
```

- [ ] **Step 5: Verify the manifests parse and the workspace still builds**

Run: `cargo build --workspace 2>&1 | tail -5`
Expected: build OK, no manifest warnings/errors.

- [ ] **Step 6: Commit**

```bash
git add LICENSE README.md Cargo.toml crates/overlay/Cargo.toml crates/lyrics/Cargo.toml crates/tuner/Cargo.toml
git commit -m "docs: add MIT license, README, and Cargo metadata for sceno

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: CI workflow

Add a GitHub Actions workflow running fmt + clippy + test with the required system deps.

**Caveat:** CI cannot be validated locally (it only runs once pushed, and we stop before pushing). The workflow is best-effort against the known dependency set; the owner validates it on the first push and adjusts the `apt` list if a sys-lib is missing.

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
  pull_request:

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install system dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libdbus-1-dev \
            libasound2-dev \
            libwayland-dev \
            libxkbcommon-dev

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - uses: Swatinem/rust-cache@v2

      - name: Format
        run: cargo fmt --all --check

      - name: Clippy
        run: cargo clippy --all-targets -- -D warnings

      - name: Test
        run: cargo test --workspace
```

- [ ] **Step 2: Validate the YAML locally (syntax only)**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml')); print('yaml OK')"`
Expected: `yaml OK`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add fmt + clippy + test workflow

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Final verification (after all tasks)

Run:

```bash
cargo fmt --all --check; echo "fmt-exit=$?"
cargo clippy --all-targets 2>&1 | tail -3
cargo test --workspace 2>&1 | grep -E "test result:"
cargo build --release 2>&1 | tail -3
ls target/release/lyrics target/release/tuner
git log --oneline -7
```

Expected: `fmt-exit=0`; clippy clean; all `test result:` lines `0 failed`; both release binaries present; six new commits (Tasks 1–6) plus the spec commit.

**Owner follow-ups (manual, out of scope here):**
- Spot-check the lyrics app writes config at `~/.config/sceno/lyrics/config.json` in a real session.
- Create the public `sceno` repo on GitHub, push, and confirm the CI workflow passes (adjust the `apt` list if a sys-lib is missing).
- Optionally rename the local working directory `lyrics-on-screen` → `sceno`.
