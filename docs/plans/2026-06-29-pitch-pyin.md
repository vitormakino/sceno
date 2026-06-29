# Pitch detection: switch to pYIN (robust against octave/harmonic + rumble errors)

*2026-06-29* — supersedes the clarity-gate tuning in `2026-06-29-pitch-clarity-tuning.md`.

## Why

After lowering the McLeod clarity gate didn't help, a `SCENO_DEBUG` capture from the reporter's
mic (**HyperX QuadCast S @ 48 kHz**) playing a known piano note (target E4 ≈ 330 Hz) showed the
detector jumping between the correct ~332 Hz and **garbage**: ~32 Hz (sub-bass rumble) and
~1949 Hz (an upper partial). The McLeod/MPM detector (`pitch-detection`) has no frequency bound and
was locking onto rumble or harmonics; lowering clarity only *admitted* more of that garbage. A
sensitive condenser mic on a desk makes this routine.

Rather than hand-roll DSP (band-pass + octave heuristics) on top of McLeod, adopt a purpose-built
detector — the project spec already earmarked **pYIN** (Mauch & Dixon 2014) as the future option
"if voice robustness becomes a pain point". pYIN is the standard for voice F0: it searches a bounded
range, tracks the most probable pitch (resisting octave/harmonic errors), and reports a voicing
probability.

## Change

- **Dependency:** drop `pitch-detection`; add `pyin = { version = "1.2", default-features = false }`
  (pure Rust — the BLAS/CLI features that pull C are opt-in and stay off).
- **`crates/pitch/src/detect.rs`:** `detect_frequency` now runs pYIN with `fmin = MIN_HZ` (70 Hz) /
  `fmax = MAX_HZ` (1000 Hz), so sub-bass rumble and high partials are simply outside the search and
  can't be reported. It returns the **median f0 over voiced frames** (pYIN's own voiced flag),
  robust to an outlier frame. The executor is cached in a `thread_local` (building one per 50 ms
  frame is far too slow).
- **`crates/pitch/src/capture.rs`:** `MIN_CLARITY` is now a floor on pYIN's voicing *probability*,
  set to **0.0** — pYIN reports a low probability for a clear-but-noisy voice even when it correctly
  marks the frame voiced, so a higher floor would re-break real mics. pYIN's voiced flag is the gate.
- **`Cargo.toml` (workspace):** `[profile.dev.package."*"]` and `[profile.dev.package.pitch]` set
  `opt-level = 3`. pYIN is generic, so its hot inner loops monomorphize **into `pitch`** — without
  optimizing `pitch` itself, a debug build runs pYIN at ~450 ms/frame (unusable); with it, ~24 ms
  (release ~2 ms). Our crates stay otherwise unoptimized/debuggable.

## Validation (`crates/pitch/tests/detection.rs`, all pass)

- `detects_clean_harmonic_voice_across_range`, `detects_voice_with_vibrato`,
  `detects_voice_with_light_noise` — accuracy across A2..C5 at 44.1/48 kHz.
- `noisy_voice_is_detected_at_production_clarity` — ≥90% at ~0 dB SNR (noise amp 0.2).
- `rejects_rumble_and_harmonic_locks` — **the regression for this bug**: a bright tone over heavy
  32 Hz rumble must land on the fundamental, never the rumble or an upper partial.
- `pure_noise_is_mostly_rejected…` + unit `rejects_out_of_range_subbass` — no inventing pitch.

## Notes / follow-ups
- **Run release builds** for best latency (`cargo build --release`); debug is fine now (~24 ms) but
  release is ~2 ms.
- `vocalize` gained a `SCENO_DEBUG` line logging target-vs-heard, which is how this was diagnosed.
- If sopranos above ~1000 Hz need support, raise `MAX_HZ` (costs a little pYIN compute).
- A mic input-level (VU) indicator in the overlay is still a possible UX add.
