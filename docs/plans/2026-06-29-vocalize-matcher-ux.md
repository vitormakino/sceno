# Vocalize matcher UX: flutter-tolerant hold + selectable octave matching

*2026-06-29*

Two follow-ups to the pYIN detection work, addressing user feedback that collecting felt
finicky ("tenho que segurar pra ele identificar; fica confuso até estabilizar") and a request to
also check the octave.

## 1. Flutter-tolerant hold (decay instead of reset)

Before, the `Matcher` zeroed a target's hold the instant the sung pitch left the in-tune window —
so a brief onset wobble or a vibrato swing across the edge threw away near-complete progress.

Now the hold **accumulates** in-window time (capped at `sustain_ms`, so excess can't be banked)
and **decays at `OUT_DECAY` = 2×** while out of window instead of resetting. Because the decay is
twice the accumulation, a pitch that's out of window more than ~1/3 of the time still falls to
zero and never collects — so flutter is forgiven **without** introducing false positives. Tests:
`brief_excursion_decays_but_does_not_reset`, `mostly_off_pitch_never_collects`,
`sustained_silence_decays_to_zero`, and the existing `clearly_off_pitch_is_rejected` still holds.

Note: this helps mid-note flutter/vibrato; the ~150–200 ms to *first* identify a note is physical
(analysis window fill + the instrument/voice attack settling) and isn't removed by this.

## 2. Selectable octave matching (`octave_strict`, default on)

Measured first: pYIN's absolute pitch is octave-reliable — `detects_the_exact_octave` (in
`crates/pitch/tests/detection.rs`) gets the exact octave 100% across A2..C5 at all tested noise
levels (guarded at ≥95%). So requiring the octave is precise, not flaky.

`Matcher::new(item, cents_window, sustain_ms, strict)` gained a `strict` flag:
- **strict** (default): the sung pitch must be in the *exact* octave of the target
  (`(sung − target)·100 ≤ cents_window`); chips show the octave (`Dó (C4)`).
- **folded** (off): octave-folded to pitch class as before (any octave passes) — kept for singers
  whose comfortable range differs from the reference octave.

Wired through `VocalizeConfig.octave_strict` (default `true`), a tray **"Oitava exata"** checkmark
(`Message::SetOctaveStrict`, re-arms the matcher on the current target in place), `apply_config`,
and `current_config`. Tests: `strict_mode_requires_exact_octave`, `note_label_oct_includes_octave`.

## Verify
- `cargo mac-test` green (vocalize 35 tests; pitch incl. exact-octave guard).
- Manual: with **Oitava exata** on, singing the right note an octave off should *not* collect;
  toggling it off, any octave collects. Vibrato/onset wobble should no longer reset the hold.
