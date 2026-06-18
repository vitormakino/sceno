# Design: `karaoke` scoring

**Date:** 2026-06-18
**Status:** spec (brainstorming → spec)

## Context

The `karaoke` app already renders the target melody (scrolling pitch-lane) and a live
mic cursor, and every frame it computes both the **active target note** (the bar under
the playhead) and `cents_to_target(sung_pitch, target)` — the octave-folded deviation
of the sung pitch. What it doesn't do is **keep score**.

Scoring is the retention/"viral" hook of UltraStar Deluxe, Performous, and Smule: a
number that rewards staying on pitch and makes you want another go. For us it is cheap —
the per-frame accuracy signal already exists; we only need to integrate it over time and
show it. This is roadmap item #3 from the competitive analysis.

## Goals

- A **live accuracy score** (0–100%) that rises while the sung pitch tracks the target.
- Golden notes (UltraStar `*`) weighted more, matching the genre's convention.
- A **per-song best** ("Recorde"), persisted, so beating your record is a reason to
  replay.
- All glanceable in the existing panel; no new surface or window.

## Non-goals

- No UltraStar 10000-point parity or note-by-note breakdown (a single % is enough).
- No combos / multipliers / end-of-song results screen.
- No leaderboards or sharing.
- No change to the lane rendering, pitch detection, or matching.

## Decisions (locked)

| Decision | Choice |
|----------|--------|
| Score model | **Time-integrated accuracy**: fraction of "target-active" time sung on pitch |
| Sampling clock | The existing 33 ms `Tick` (already gated to `enabled && song && !paused`) |
| Integration | By **playback-time delta** (`t - last_t`, clamped to 0.5 s) — robust to tick jitter, pauses, and seeks; idle gaps between notes don't count |
| Credit curve | Linear partial credit: `quality = (1 - |cents|/50).clamp(0,1)` — perfect at 0¢, zero past ±50¢ |
| Golden notes | Weight ×2 in both earned and possible (golden math is `pitch::cents_to_target` agnostic) |
| Score = | `earned / possible * 100`, `None` until any target time has elapsed |
| Reset | On track **change** (only when the track key actually changes) |
| Best persistence | `KaraokeConfig.best_scores: HashMap<TrackQuery::key, f64>`, committed on track change and on disable |
| Display | Readout gains `Pontos: NN%` (colored by grade) and `Recorde: NN%` for the song |

### Why time-integration, not per-note points

Per-frame fixed points couple the score to the (jittery) tick rate and mishandle
pauses/seeks. Integrating the accuracy over **playback time** with a clamped delta makes
the score independent of frame cadence: a 3-minute song sung perfectly is 100% whether
the ticks were even or not, and a seek backward doesn't inflate it.

## New module: `crates/karaoke/src/score.rs`

Pure and unit-tested (no audio/UI):

```rust
/// Largest playback-time step counted per sample; bigger jumps (seek/resume from
/// pause) are clamped so they neither inflate nor tank the score.
const MAX_DT: f64 = 0.5;

#[derive(Default)]
pub struct Scorer {
    earned: f64,
    possible: f64,
    last_t: Option<f64>,
}

impl Scorer {
    pub fn reset(&mut self) { *self = Scorer::default(); }

    /// Accumulate one frame at playback time `t`. `has_target` is whether a note is
    /// active now; `golden` weights it ×2; `sung_cents` is the octave-folded
    /// deviation of the sung pitch (None when silent / no target).
    pub fn sample(&mut self, t: f64, has_target: bool, golden: bool, sung_cents: Option<f64>) {
        let dt = match self.last_t { Some(p) if t > p => (t - p).min(MAX_DT), _ => 0.0 };
        self.last_t = Some(t);
        if dt <= 0.0 || !has_target { return; }
        let w = if golden { 2.0 } else { 1.0 };
        self.possible += dt * w;
        if let Some(c) = sung_cents {
            self.earned += dt * w * (1.0 - c.abs() / 50.0).clamp(0.0, 1.0);
        }
    }

    /// Score so far (0–100), or None before any target time elapsed.
    pub fn pct(&self) -> Option<f64> {
        (self.possible > 0.0).then(|| self.earned / self.possible * 100.0)
    }
}
```

## App changes (`crates/karaoke/src/main.rs`)

- `State` gains `scorer: Scorer`, `track_key: Option<String>`, and `best:
  HashMap<String, f64>` (mirrors the persisted map).
- **`Tick`** (today a no-op): compute the active target + golden flag from
  `song`/`current_time`, derive `sung_cents` from `current_note`, call
  `scorer.sample(...)`, then bump the live best. Field-disjoint borrows: compute the
  target into owned locals first, then call `&mut self.scorer`.
- **`TrackChanged`**: keep the unconditional `paused`/`sync` update; gate song reload +
  `scorer.reset()` + `track_key` on the key actually changing, and `commit_best()` for
  the song being left first (so re-emitted same-track events don't wipe progress).
- **`SetEnabled(false)`**: `commit_best()` so a record isn't lost when hiding the panel.
- `commit_best()`: `best.entry(key).max(pct)`, persist when it improves.
- `persist()` writes `best_scores`.

### Config (`config.rs`)

Add `best_scores: HashMap<String, f64>` with `#[serde(default)]` so existing configs
load; extend the roundtrip + defaults tests.

### Display (`view`)

The readout row gains a score chip colored by grade (≥90 green, ≥70 amber, else dim)
and a dim `Recorde: NN%` (the live max of stored best and current). When no target time
has elapsed yet, show `Pontos: —`.

## Testing

Pure/deterministic:

- **Scorer:** seed at `t0` (first sample is dt=0, no-op), then:
  - perfect singing (`sung_cents = 0`) over a target → `pct ≈ 100`.
  - silent (`sung_cents = None`) over a target → `pct == 0`.
  - half-off (`|cents| = 25`) → `pct ≈ 50`.
  - no target the whole time → `pct() == None`.
  - golden weighting: a missed normal note + a hit golden note → `pct ≈ 66.7%`.
  - a big `t` jump is clamped (one sample adds ≤ `MAX_DT·w` to `possible`).
  - `reset()` zeroes it.
- App: `commit_best` records the max and is idempotent; a same-key `TrackChanged`
  doesn't reset the scorer; a new key does.
- Existing karaoke tests stay green.

## Verification

- `cargo +stable fmt --all --check`, `clippy --all-targets -- -D warnings`,
  `test --workspace` green.
- Manual: play a song in the library; sing along — `Pontos` climbs toward 100% while on
  pitch, stalls when silent/off; switch songs and back — `Recorde` shows the prior best;
  beat it and confirm it persists across a restart.

## Out of scope (tracked)

- Quitting mid-song via the tray's hard `exit(0)` won't capture a best (no hook); the
  record is saved on track change / disable. A graceful-quit hook is separate work.
- End-of-song results screen, combos, star rating, online scores.
