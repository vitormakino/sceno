# Pitch detection: voice robustness (clarity gate) + detection integration tests

*2026-06-29*

## Why

Reported symptom: `vocalize` "isn't picking up my singing — either I sing badly or it's not
detecting." Needed to determine empirically whether the fault was the *algorithm*, the *matcher
chain*, or the *real-signal capture*, and add objective tests rather than guess.

## Investigation (all via new synthetic tests, no mic)

1. **Algorithm** — `crates/pitch/tests/detection.rs` drives the real `detect_frequency`
   (McLeod/MPM) with voice-like signals (fundamental + decaying harmonics, phase-accurate
   vibrato, white noise), swept A2..C5 at 44.1 and 48 kHz. **Clean, ±35¢ vibrato, and lightly
   noisy tones all detect correctly** — the detector itself is solid.

2. **Matcher chain** — a simulation in `crates/vocalize/src/exercise.rs` tests
   (`pitch::Smoother` → `frequency_to_note` → `Matcher`): on-pitch, wide vibrato (the EMA averages
   it toward center), octave-up, and modestly-off all collect at ~the sustain time; only a clearly
   off-pitch (55¢ flat, past the ±50¢ window) is rejected. **The chain is fine and tolerant.**

3. **Real-signal capture — the culprit.** A detection-rate sweep over noise level × clarity
   threshold showed the production gate was far too strict:

   | noise | clar 0.6 | 0.5 | 0.45 | 0.4 | 0.3 |
   |------:|---------:|----:|-----:|----:|----:|
   | 0.30  |    0%    | 61% | 100% |100% |100% |
   | 0.35  |    0%    |  0% |  11% | 98% |100% |

   At a realistic noisy/low-level mic (noise ≈ signal), `MIN_CLARITY = 0.6` rejected **100%** of
   frames — detection went silent and nothing ever collected, exactly the reported symptom.

## Change

- **`crates/pitch/src/capture.rs`: `MIN_CLARITY` 0.6 → 0.4.** Recovers noisy-but-voiced signal
  (up to noise ≈ 0.35) while still rejecting heavier noise (≥0.4 → 0% detection) and pure noise,
  so it doesn't invent a pitch from silence. Shared by `tuner`/`karaoke`/`vocalize` — all benefit
  from better real-room sensitivity; the `Smoother` hold already damps the slightly-higher jitter.
- **`crates/pitch/tests/detection.rs`** (new integration test): voice-sweep accuracy (clean /
  vibrato / light noise), a `noisy_voice_is_detected_at_production_clarity` regression (≥90% at
  noise 0.3 — would fail at the old 0.6), and `pure_noise_is_mostly_rejected_at_production_clarity`
  (≤20% false positives) so the lower gate can't regress into detecting silence.
- **`crates/vocalize/src/exercise.rs`** (new tests): end-to-end matcher-chain assertions
  (steady-on-pitch collects promptly, vibrato tolerated, octave-folding, clearly-off rejected).

## Out of scope (follow-ups if real-world testing still struggles)
- pYIN / `loqa-voice-dsp` (probabilistic, voiced-probability gate) — the spec's noted future
  option for breathy/noisy voice; only worth it if 0.4 proves insufficient on real mics.
- Exposing clarity / a mic input-level (VU) indicator in the overlay so the user can see whether
  the mic is being heard.
- Adaptive/noise-floor-relative clarity gating.

## Verify
- `cargo mac-test` (incl. `cargo test -p pitch --test detection`) — green.
- Real check (owner's mic): sing into `vocalize`; the `Você:` readout should now track the voice
  and chips should turn green when held on pitch, even in a normal (non-silent) room.
