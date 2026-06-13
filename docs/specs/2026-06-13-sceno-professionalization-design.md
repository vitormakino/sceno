# Design: `sceno` professionalization pack

**Date:** 2026-06-13
**Status:** approved (brainstorming → spec)

## Context

The project began as `lyrics-on-screen` (a Wayland layer-shell overlay that scraped
YouTube captions) and has since become a small **Cargo workspace** — a shared `overlay`
base crate plus two app crates (`lyrics`, `tuner`), with a metronome envisioned later.
The data layer was rebuilt natively (MPRIS + LRCLIB) and the vocal tuner shipped. The
code is healthy (63 tests, clippy-clean) but **not yet presentable as a public repo**:
no name that fits a multi-app workspace, no LICENSE/README, no CI, hardcoded non-XGD
paths, and a minor retry bug.

This pack makes the workspace publishable. It is a sequence of small, independent
commits. The goal is to **prepare** for publishing and stop before the actual repo
creation / push — the owner runs the final publish.

## Decisions (locked)

| Decision | Choice |
|----------|--------|
| Workspace/repo name | **`sceno`** (from "scene" — fits a screen-overlay toolkit) |
| License | **MIT** |
| Public identity | **`Vitor Makino <vitor.makino@gmail.com>`** (personal email kept by choice) |
| Repository URL | `https://github.com/vitormakino/sceno` |
| Scope | Prepare everything; **stop before** creating the GitHub repo / pushing |
| Git history rewrite | **Not done** — email is kept, so no identity scrub is needed |

Binaries stay named `lyrics` and `tuner`. The local working directory is still
`lyrics-on-screen`; renaming it to `sceno` is an optional filesystem move the owner can
do anytime — out of scope here (risky from inside the running session, not required by
the code).

## Work items

Each numbered item is its own commit, performed in order. Items A1–A4 are
code/config; B and C add project files; there is no history-rewrite step.

### A1. Fix `call_with_retry` (only retry transient failures)

`crates/lyrics/src/lrclib.rs`. Today it returns immediately on `404` but retries every
other error — including non-transient `4xx` (e.g. `400`, `403`). Change it so **any
`4xx` is a definitive result returned immediately**; retry only `5xx` and transport/
network errors. Update the doc comment to match. Add a test asserting a `4xx` (e.g.
`400`) is not retried (the existing 404 behavior stays).

### A2. XDG-compliant paths + `sceno` basename

Today config lives at `~/.config/lyrics-on-screen/config.json`
(`overlay/src/settings.rs`) and the cache at `~/.cache/lyrics-on-screen/`
(`lyrics/src/lrclib.rs`), both hardcoding `$HOME` and the old name.

- Add two helpers to `crates/overlay` (the shared base):
  - `config_dir(app: &str) -> Option<PathBuf>` → `$XDG_CONFIG_HOME/sceno/<app>`
    (fallback `~/.config/sceno/<app>`).
  - `cache_dir(app: &str) -> Option<PathBuf>` → `$XDG_CACHE_HOME/sceno/<app>`
    (fallback `~/.cache/sceno/<app>`).
  - Both honor the XDG env var when set and non-empty, else fall back to `$HOME/...`.
- Thread the **app name** through the config API so apps don't collide: change
  `load_config` / `save` to take an `app: &str` (lyrics passes its namespace `"lyrics"`).
  New config path: `~/.config/sceno/lyrics/config.json`.
- Point the lyrics cache at `cache_dir("lyrics")` → `~/.cache/sceno/lyrics/`.
- **No migration shim** (single user, trivial config). Old `~/.config/lyrics-on-screen`
  is simply orphaned; the owner can `mv` it or let settings reset to defaults.

### A3. Migrate to edition 2024

Bump `Cargo.toml` `workspace.package.edition = "2024"`. Run `cargo fix --edition`
(rustc 1.94 supports 2024), apply any idiom fixes, and verify build / test / clippy stay
green.

### A4. `cargo fmt --all` sweep

One mechanical commit normalizing the whole workspace to rustfmt — the formatting debt
deliberately deferred from the tuner branch. Done after A1–A3 so it formats the new code
too.

### B. LICENSE, README, Cargo metadata (one commit)

- **`LICENSE`** — MIT, `Copyright (c) 2026 Vitor Makino`.
- **`README.md`** — concise: what `sceno` is (a small family of minimal Wayland
  layer-shell overlay apps), the apps (`lyrics` = synced lyrics via MPRIS + LRCLIB;
  `tuner` = mic → note + tuning meter), build dependencies
  (`libdbus-1-dev`, `libasound2-dev`, plus Wayland/xkb sys-libs), build & run
  instructions, license.
- **Cargo metadata** in `workspace.package`: `description`, `license = "MIT"`,
  `repository = "https://github.com/vitormakino/sceno"`,
  `authors = ["Vitor Makino <vitor.makino@gmail.com>"]`,
  `readme = "README.md"`, `keywords`, `categories`. Per-crate `description` for
  `overlay` / `lyrics` / `tuner`.

### C. CI workflow (one commit)

`.github/workflows/ci.yml` — on `push` and `pull_request`, Ubuntu runner:

1. `apt-get install -y libdbus-1-dev libasound2-dev libwayland-dev libxkbcommon-dev`
   (the build deps for `mpris`, `cpal`, and `iced_layershell`'s Wayland stack;
   additional sys-libs may surface on first run).
2. `cargo fmt --all --check`
3. `cargo clippy --all-targets -- -D warnings`
4. `cargo test --workspace`

**Caveat:** CI cannot be validated locally without pushing (and we stop before pushing).
The workflow is best-effort against the known dependency set; the owner validates it on
the first push and we adjust the `apt` list if a sys-lib is missing.

## Verification

After each item: `cargo build --workspace`, `cargo test --workspace`,
`cargo clippy --all-targets` stay green; `cargo fmt --all --check` clean after A4.
Final release build produces both `lyrics` and `tuner` binaries. The lyrics app still
reads/writes its config at the new XDG path (owner spot-checks in a real session).

## Out of scope

- Creating the GitHub repo / pushing (owner does this).
- Renaming the local working directory.
- Removing `cosmic-osd*` files (already gitignored, untracked).
- Generic tray extraction, autostart, A/V offset, metronome app (separate future work).
