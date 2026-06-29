# Vocalize — more scales + per-note practice statistics

*2026-06-29*

## Why

Product roadmap pass for the `sceno` family: `vocalize` shipped with only three scales
(Major / Natural minor / Chromatic) and no notion of progress, while competing ear-trainers
(Perfect Ear, Functional Ear Trainer, Vanido) offer the church modes and per-exercise feedback.
Two owner-chosen increments close that gap:

1. **More scales** — the seven diatonic-family additions the owner picked: the church modes
   (Dorian, Phrygian, Lydian, Mixolydian, Locrian) plus the harmonic and melodic minors.
2. **Per-note statistics** — track which pitch classes the singer takes longest to nail, so
   practice can target weak spots (the owner picked "estatísticas por nota" over session
   streaks / timed gamification).

The earlier vocalize spec listed "scoring/streaks/history" as out-of-scope-for-now; this is the
deliberate revisit of that, scoped to *per-note timing* rather than gamified scoring.

## What changed

### Scales (`crates/vocalize/src/exercise.rs`)
- `ScaleKind` gained `Dorian, Phrygian, Lydian, Mixolydian, Locrian, HarmonicMinor, MelodicMinor`
  (appended after the existing three — indices are persisted, so order is append-only). Each adds
  a `degrees()` arm, a `label()`, an `index()`/`from_idx()` arm, and an `ALL` entry.
- No tray change needed: `vocalize/src/tray.rs` builds the **Escala** radio group from
  `ScaleKind::ALL`, so the seven new kinds appear automatically.
- Melodic minor is the **ascending** form (`[0,2,3,5,7,9,11]`); descending/jazz variants are out
  of scope. Tests assert the characteristic tones (Lydian #4, Mixolydian b7, harmonic-minor raised
  7th) and that every kind's index/label/ALL stay aligned.

### Per-note stats (`crates/vocalize/src/stats.rs`, new)
- `Stats` accumulates `(count, total_ms)` per pitch class (0–11). `record(class, ms)`,
  `avg_ms(class)`, `hardest()` (highest average with ≥`MIN_SAMPLES`=3 samples), `clear()`.
- **Stored outside the config** — `<data_dir>/vocalize/stats.json` via `overlay::data_dir`, loaded
  on startup, saved on each collection — so "Restaurar padrões" (which resets the *config*) never
  wipes practice history. Load/save are best-effort (malformed/missing → defaults).
- **Measurement** (`main.rs`): an `item_armed_at: Instant` is set when the present (listen) phase
  ends — i.e. when the target becomes matchable — and on a silent structural config change. In the
  `Tick` handler, each index the `Matcher` reports as newly collected is recorded as
  `(item[i] pitch class, now − item_armed_at)`. Chords record each note's own elapsed-since-arm.
- **Display**: a dim `⌛ mais difícil: <solfège> (~N.Ns)` line under the readout in the overlay,
  shown only once some class has ≥3 samples. The overlay updates live, so this is where the
  feedback lives (the ksni tray is built once and isn't live-updated).
- **Reset**: tray **"Limpar estatísticas"** → `Message::ResetStats` → `Stats::clear()` + save
  (Linux only; on macOS, delete the JSON file).

### Deps
- `serde_json` moved from dev- to a normal dependency of `vocalize` (used by `stats` load/save).

## Verification
- **Unit (CI both jobs):** `exercise` — degree counts, characteristic mode tones, index/label/ALL
  alignment, idx roundtrip (now covers all 10 kinds). `stats` — record/average, octave-folding to
  pitch class, `hardest` honoring `MIN_SAMPLES` and picking the slowest, `clear`, JSON roundtrip.
- **macOS:** `cargo mac{,-clippy -- -D warnings,-test}` green.
- **Manual:** pick a mode in the tray → targets come from that scale; sing several items → the
  `⌛ mais difícil` line appears and points at the slowest pitch class; "Limpar estatísticas"
  clears it; "Restaurar padrões" does **not**.

## Out of scope (follow-ups)
- Session streaks / timed gamification / a "complete N in a row" goal.
- Descending or jazz melodic-minor; pentatonic/blues (not picked this pass).
- A full stats panel / history graph; live stats in the tray (needs the tray `Handle`, the
  existing tray-desync follow-up).
