# Overlay Workspace Extraction — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the single-crate lyrics app into a Cargo workspace with a reusable `overlay` library, so a second app (the vocal tuner) can share the Wayland layer-shell plumbing without duplication.

**Architecture:** A `crates/overlay` library owns the generic shell — layer-shell surface, transparent/click-through window, the standard tray (enabled / position / font size / quit), config persistence (font/position/enabled), and a runner that wraps `iced_layershell::application`. Each app supplies its own iced `State`/`update`/`view`; the runner injects the shared pieces. `crates/lyrics` is the current app refactored onto that library, with its lyric-specific timeline logic (`CueEntry`, `TimelineSync`, `cue_at`, MPRIS/LRCLIB) staying in the binary.

**Tech Stack:** Rust, Cargo workspace, `iced` 0.14, `iced_layershell` 0.18, `ksni`, `serde`/`serde_json`.

**Reference:** spec at `docs/specs/2026-06-12-vocal-tuner-design.md`.

> **Note on precision:** the exact `overlay` runner API depends on how `iced_layershell`'s `application(...)` + `#[to_layer_message]` macro compose a shared+app message type. Task 2 resolves that against the compiler and **locks the real signatures**; later tasks follow whatever Task 2 establishes. Where this plan shows an interface, treat it as the target to make compile, not a guess to copy blindly.

---

## File Structure (target)

```
Cargo.toml                         # [workspace] members = ["crates/*"]
crates/
  overlay/
    Cargo.toml
    src/
      lib.rs                       # pub re-exports + OverlayApp trait + run()
      settings.rs                  # Position, FontSize, SavedConfig, load/save
      tray.rs                      # shared ksni tray + the messages it emits
      single_instance.rs          # ensure_single_instance(lock_name)
  lyrics/
    Cargo.toml
    src/
      main.rs                      # State/update/view for captions; calls overlay::run
      lrc.rs                       # (moved as-is)
      lrclib.rs                    # (moved as-is)
      player.rs                    # (moved as-is)
docs/...                           # specs + plans (unchanged)
```

The current root `src/` is removed; its generic parts move to `crates/overlay`, its lyric parts to `crates/lyrics`.

---

## Task 1: Create the workspace skeleton and move lyrics unchanged

Goal: convert the repo to a workspace with a single `lyrics` member that still builds and passes tests — pure move, no behavior change. This de-risks the layout before any extraction.

**Files:**
- Create: `Cargo.toml` (workspace root, replaces the current package manifest)
- Create: `crates/lyrics/Cargo.toml`
- Move: `src/main.rs` → `crates/lyrics/src/main.rs`
- Move: `src/lrc.rs`, `src/lrclib.rs`, `src/player.rs` → `crates/lyrics/src/`

- [ ] **Step 1: Move the sources with git**

```bash
mkdir -p crates/lyrics/src
git mv src/main.rs crates/lyrics/src/main.rs
git mv src/lrc.rs crates/lyrics/src/lrc.rs
git mv src/lrclib.rs crates/lyrics/src/lrclib.rs
git mv src/player.rs crates/lyrics/src/player.rs
```

- [ ] **Step 2: Write the workspace root `Cargo.toml`**

Replace the entire root `Cargo.toml` with:

```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
edition = "2021"
version = "0.1.0"

[workspace.dependencies]
iced = "0.14"
iced_layershell = "0.18"
futures = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ksni = "0.2"
libc = "0.2"
mpris = "2"
ureq = "2"
```

- [ ] **Step 3: Write `crates/lyrics/Cargo.toml`**

```toml
[package]
name = "lyrics"
version.workspace = true
edition.workspace = true

[dependencies]
iced.workspace = true
iced_layershell.workspace = true
futures.workspace = true
serde.workspace = true
serde_json.workspace = true
ksni.workspace = true
libc.workspace = true
mpris.workspace = true
ureq.workspace = true
```

- [ ] **Step 4: Build and test (must pass unchanged)**

Run: `cargo test`
Expected: PASS — `test result: ok. 55 passed` (same suite as before the move).

Run: `cargo clippy --all-targets`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: convert to Cargo workspace, move app into crates/lyrics"
```

---

## Task 2: Create the `overlay` crate and establish the runner API (the spike)

Goal: stand up `crates/overlay` and move the **most entangled** generic piece — the `iced_layershell` application setup — behind a runner that `lyrics` calls, proving the composition of a shared+app message type compiles and runs identically. This task locks the real `OverlayApp` trait / `run` signature that the rest of the plan and the future tuner depend on.

**Files:**
- Create: `crates/overlay/Cargo.toml`
- Create: `crates/overlay/src/lib.rs`
- Modify: `crates/lyrics/src/main.rs` (call `overlay::run` instead of `iced_layershell::application` directly)
- Modify: `crates/lyrics/Cargo.toml` (depend on `overlay`)

- [ ] **Step 1: Write `crates/overlay/Cargo.toml`**

```toml
[package]
name = "overlay"
version.workspace = true
edition.workspace = true

[dependencies]
iced.workspace = true
iced_layershell.workspace = true
ksni.workspace = true
serde.workspace = true
serde_json.workspace = true
libc.workspace = true
```

- [ ] **Step 2: Define the `OverlayApp` trait + `run` in `crates/overlay/src/lib.rs`**

Target interface (make this compile against `iced_layershell` 0.14/0.18; adjust the
exact bounds/associated-type plumbing as the compiler requires — this signature is
the contract the lyrics app and the tuner will implement):

```rust
//! Reusable Wayland layer-shell overlay shell shared by the overlay apps.

use iced::Element;

/// One overlay application: provides its own message/state/update/view; the
/// runner injects the layer-shell surface, the shared tray, and config.
pub trait OverlayApp: Sized + 'static {
    /// The app's own message type (must carry the layer-shell control variants
    /// via `#[to_layer_message]`, like the lyrics `Message` does today).
    type Message: Clone + std::fmt::Debug + Send + 'static;

    fn namespace() -> &'static str;
    fn new() -> Self;
    fn update(&mut self, message: Self::Message) -> iced::Task<Self::Message>;
    fn view(&self) -> Element<'_, Self::Message>;
    fn subscription(&self) -> iced::Subscription<Self::Message>;
}

/// Run an overlay app to completion (owns the process event loop).
pub fn run<A: OverlayApp>() -> iced_layershell::Result {
    // Wraps `iced_layershell::application(A::new, A::namespace(), update, view)`
    // with the shared style (transparent bg, white text) and the standard
    // LayerShellSettings (bottom-anchored, Top layer, size (0,80), transparent,
    // events_transparent, keyboard None) currently in `lyrics/src/main.rs::main`.
}
```

- [ ] **Step 3: Move the layer-shell `main` wiring from lyrics into `overlay::run`**

In `crates/lyrics/src/main.rs`, replace the body of `fn main()` (the
`iced_layershell::application(...)...run()` chain at the current `main`) with:

```rust
fn main() -> iced_layershell::Result {
    overlay::run::<State>()
}
```

…and implement `OverlayApp for State` in `lyrics/src/main.rs`, delegating to the
existing `update`/`view`/`subscription`/`State::default` functions. Move the
`LayerShellSettings`, the `.style(...)` closure, and `ensure_single_instance()`
invocation into `overlay::run`. Keep the lyric `Message`'s `#[to_layer_message]`.

- [ ] **Step 4: Add the dependency**

In `crates/lyrics/Cargo.toml` add under `[dependencies]`:

```toml
overlay = { path = "../overlay" }
```

- [ ] **Step 5: Build, test, and run identically**

Run: `cargo test`
Expected: PASS — all existing lyrics tests still green.

Run: `cargo build`
Expected: builds clean, no warnings.

Manual: `LOS_DEBUG=1 ./target/debug/lyrics` with music playing → synced lyrics appear exactly as before. (Note the binary is now `lyrics`, not `lyrics-on-screen`.)

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor: move layer-shell runner into overlay crate"
```

---

## Task 3: Move shared settings (Position, FontSize, config) into `overlay::settings`

Goal: the generic position/font/enabled types and their JSON persistence belong to the library, not the lyrics binary.

**Files:**
- Create: `crates/overlay/src/settings.rs`
- Modify: `crates/overlay/src/lib.rs` (`mod settings; pub use ...`)
- Modify: `crates/lyrics/src/main.rs` (use `overlay::{Position, FontSize, ...}`, drop the local copies)

- [ ] **Step 1: Move the types verbatim into `crates/overlay/src/settings.rs`**

Cut these items from `lyrics/src/main.rs` into `settings.rs` and make them `pub`:
`Position` (+ `anchor`/`margin`/`index`), `FontSize` (+ `px`/`index`/`from_idx`),
`SavedConfig` (+ `default_font_idx`/`default_enabled`/`Default`), `config_path`,
`load_config`. Replace `save_config(&State)` with a generic:

```rust
pub fn save(font_size: FontSize, position: Position, enabled: bool) {
    if cfg!(test) { return; }
    // …existing create_dir_all + serde_json::to_string + fs::write to config_path()…
}
```

Make the config carry `position_idx` too (currently `Position` is not persisted —
add it now so it round-trips). Keep the existing `mode_idx`-tolerance test behavior
(unknown fields ignored).

- [ ] **Step 2: Re-export from `lib.rs`**

```rust
mod settings;
pub use settings::{FontSize, Position, SavedConfig, load_config, save};
```

- [ ] **Step 3: Update lyrics to use the library types**

In `lyrics/src/main.rs`, delete the moved definitions and add
`use overlay::{FontSize, Position};` (and `overlay::load_config`/`overlay::save`
where `load_config`/`save_config` were used). Move the corresponding
`SavedConfig`/`FontSize`/`Position` unit tests into `crates/overlay/src/settings.rs`.

- [ ] **Step 4: Test**

Run: `cargo test`
Expected: PASS — settings tests now run in the `overlay` crate; lyrics tests still pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: move Position/FontSize/config into overlay::settings"
```

---

## Task 4: Move the shared tray + single-instance into `overlay`

Goal: the tray (enabled / position / font size / quit) and the file lock are generic. The tray must emit messages the app handles; parameterize the lock name per app so two apps can run at once.

**Files:**
- Create: `crates/overlay/src/tray.rs`, `crates/overlay/src/single_instance.rs`
- Modify: `crates/overlay/src/lib.rs`, `crates/lyrics/src/main.rs`

- [ ] **Step 1: Parameterize the lock by app name in `single_instance.rs`**

```rust
use std::os::unix::io::AsRawFd;

/// Acquire a per-app exclusive lock; exits 0 if another instance holds it.
pub fn ensure_single_instance(app: &str) {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let path = std::path::Path::new(&dir).join(format!("{app}.lock"));
    let file = std::fs::OpenOptions::new()
        .write(true).create(true).truncate(false).open(&path)
        .unwrap_or_else(|e| { eprintln!("[overlay] lock open failed: {e}"); std::process::exit(1); });
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret != 0 { eprintln!("[overlay] {app} já está em execução"); std::process::exit(0); }
    std::mem::forget(file);
}
```

`overlay::run::<A>()` calls `ensure_single_instance(A::namespace())`.

- [ ] **Step 2: Move the tray into `tray.rs`, parameterized by a callback**

Move `LyricsTray` → a generic `OverlayTray` whose `menu()` builds the shared items
(Overlay ativo / Posição / Tamanho da fonte / Sair). It holds an
`mpsc::UnboundedSender<T>` and three closures (or a small `TrayMsg` enum) so the app
maps tray actions to its own message type. Concretely, define:

```rust
pub enum TrayEvent {
    SetEnabled(bool),
    SetPosition(Position),
    SetFontSize(FontSize),
}
```

The tray sends `TrayEvent`s; `overlay::run` translates them into the app's
layer-shell/control messages and the app's `SetEnabled/SetFontSize` handlers.

- [ ] **Step 3: Wire lyrics to the shared tray**

Delete `LyricsTray` from `lyrics/src/main.rs`; its `event_stream` keeps spawning
`player::run`, but the tray is now spawned by `overlay::run`.

- [ ] **Step 4: Build, test, run**

Run: `cargo test` → PASS.
Manual: run `lyrics`, exercise the tray (toggle overlay, change position/font) → behaves as before.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: move tray + single-instance lock into overlay crate"
```

---

## Task 5: Trim the lyrics binary and confirm the seam

Goal: `crates/lyrics/src/main.rs` now contains only lyric-specific code (caption `State`, the cue/sync `Message` variants, `apply_timeline_caption`, `view` rendering the caption, `event_stream`/`timeline_tick_stream`) plus `impl OverlayApp for State`. Everything generic lives in `overlay`.

**Files:**
- Modify: `crates/lyrics/src/main.rs`

- [ ] **Step 1: Review what remains; move any stragglers**

Confirm `lyrics/src/main.rs` no longer defines `Position`, `FontSize`,
`SavedConfig`, the tray, or the lock. Anything generic still here moves to `overlay`.

- [ ] **Step 2: Full verification**

Run: `cargo test` → all crates PASS.
Run: `cargo clippy --all-targets` → clean.
Run: `cargo build --release` → builds.
Manual: `./target/release/lyrics` with a track playing → synced lyrics identical to pre-refactor.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor: lyrics binary now only holds caption logic on overlay base"
```

---

## Self-Review (done while writing)

- **Spec coverage:** the spec's "extract overlay lib", "refactor lyrics onto it", and "keep each binary small" are covered by Tasks 1–5. The tuner app and its `cpal`/`pitch-detection` work are intentionally a **separate Plan 2** (written once this lands and `OverlayApp` is concrete). The spec's "Position not persisted" gap is fixed in Task 3.
- **Placeholder note:** the `overlay::run` body and the exact `OverlayApp` bounds are deliberately specified as a contract resolved against the compiler in Task 2 (iced_layershell composition cannot be honestly pinned to exact code without building it). Every other task uses concrete, existing code.
- **Type consistency:** `OverlayApp`/`run`/`TrayEvent`/`ensure_single_instance(app)`/`overlay::save(...)` are referenced consistently across tasks.

---

## Next plan (not this one)

**Plan 2 — Vocal tuner app** (`crates/tuner`): `cpal` capture → `pitch-detection` →
note + cents → meter `view`, implementing `OverlayApp` against the API locked in
Task 2. Written after Plan 1 merges.
