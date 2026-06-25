# vocalize electric-piano timbre — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `vocalize`'s reference tone a richer **electric-piano** timbre via pure-Rust FM synthesis, selectable from the tray (`Timbre`: Piano elétrico / Senóide), defaulting to the new sound.

**Architecture:** A `Timbre` enum + FM-voice rendering live in `crates/vocalize/src/tone.rs` (the synthesis module stays self-contained — no dependency on `exercise`). `ToneSynth` gains a second phase per voice (carrier + modulator) and a percussive amplitude envelope for the EP voice; the `Sine` voice keeps today's behavior. `Tone::play` and the persisted config/tray carry the chosen timbre, mirroring the existing `Mode`/`PlayStyle` wiring.

**Tech Stack:** Rust edition 2024, `cpal` output stream, `iced_layershell`, `ksni` tray, `serde` config. No new dependencies, no shipped assets.

## Global Constraints

- Pure Rust, **no new dependencies, no asset files** (FM synthesis only).
- `Timbre` lives in `tone.rs` (a synthesis concern); `config`/`main`/`tray` reference `tone::Timbre`.
- Stable persisted indices: `ElectricPiano = 0` (default), `Sine = 1`.
- `vocalize` is a **binary** crate, so unused `pub` items flag as `dead_code`; the plan is ordered so each task wires its new symbols within the same task — do NOT use `#[allow(...)]` (per CLAUDE.md). `Timbre::index`/`from_idx` are consumed in Task 1; `Timbre::label`/`ALL` are added in Task 2 (their tray consumer lands there too).
- `#[to_layer_message]` injects `Message` variants, so the `update` match keeps its `_ => {}` arm.
- The `Sine` timbre must reproduce the CURRENT sound exactly (summed sines, linear attack/release ramp via `RAMP_SECS`, flat middle, `/ n` anti-clip, `* GAIN`).
- Durations are unchanged: `NOTE_SECS` for a single note or a `together` chord, `ARP_SECS * len` for an arpejo. The matcher's `present_until` gate is unaffected.
- CI gate (Rust 1.96 stable): `cargo +stable fmt --all --check`, `cargo +stable clippy --all-targets -- -D warnings`, `cargo +stable test --workspace`.

---

### Task 1: `Timbre` type + FM electric-piano voice in `tone.rs`, wired through config + main

The synthesis core and just enough plumbing to keep the crate compiling and playing the EP voice by default. No tray selector yet (that is Task 2), but everything here is consumed, so the strict clippy gate passes.

**Files:**
- Modify: `crates/vocalize/src/tone.rs`
- Modify: `crates/vocalize/src/config.rs`
- Modify: `crates/vocalize/src/main.rs`

**Interfaces:**
- Produces:
  - `tone::Timbre { ElectricPiano, Sine }` with `index() -> usize`, `from_idx(usize) -> Self` (derives `Debug, Clone, Copy, PartialEq, Eq`).
  - `tone::Tone::play(&self, freqs: &[f64], together: bool, timbre: Timbre) -> Duration`.
  - `config::VocalizeConfig.timbre_idx: usize`.
  - `State.timbre: Timbre`.
- Consumes: existing `tone::Tone`, `ToneSynth`, `freqs_of`, `PlayStyle`.

- [ ] **Step 1: Write the failing `tone.rs` tests**

Append this test module to the END of `crates/vocalize/src/tone.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timbre_idx_roundtrips() {
        assert_eq!(
            Timbre::from_idx(Timbre::ElectricPiano.index()),
            Timbre::ElectricPiano
        );
        assert_eq!(Timbre::from_idx(Timbre::Sine.index()), Timbre::Sine);
        assert_eq!(Timbre::from_idx(99), Timbre::ElectricPiano);
    }

    fn render(timbre: Timbre, n: usize) -> Vec<f32> {
        let mut s = ToneSynth::new(48_000.0);
        // A C-major triad played together.
        s.load(&[261.63, 329.63, 392.0], true, timbre);
        (0..n).map(|_| s.next()).collect()
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, &x| m.max(x.abs()))
    }

    #[test]
    fn output_is_finite_and_bounded() {
        for timbre in [Timbre::ElectricPiano, Timbre::Sine] {
            let out = render(timbre, 48_000); // ~1 s
            assert!(
                out.iter().all(|x| x.is_finite()),
                "{timbre:?} produced a non-finite sample"
            );
            assert!(
                out.iter().all(|x| x.abs() <= 1.0),
                "{timbre:?} clipped past 1.0"
            );
            assert!(peak(&out) > 0.0, "{timbre:?} was silent");
        }
    }

    #[test]
    fn electric_piano_decays() {
        let all = render(Timbre::ElectricPiano, 48_000);
        let early = peak(&all[0..2_400]); // ~0..50 ms
        let late = peak(&all[33_600..36_000]); // ~700..750 ms
        assert!(
            early > late * 1.5,
            "EP did not decay: early {early}, late {late}"
        );
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail (do not compile)**

Run: `cargo +stable test -p vocalize --lib 2>&1 | tail -5` (or just `cargo +stable build -p vocalize`)
Expected: FAIL — `Timbre` not found, `load` takes 2 args not 3, etc. (compile errors). This confirms the tests reference the not-yet-built API.

- [ ] **Step 3: Add `Timbre` + EP constants + extend `Pending` in `tone.rs`**

In `crates/vocalize/src/tone.rs`, replace the existing constants + `Pending` alias block:

```rust
/// Attack / release ramp (s), to avoid clicks.
const RAMP_SECS: f64 = 0.012;

/// Shared slot for the next play request: the frequencies and whether to sound them
/// together (block chord) or one after another (arpejo).
type Pending = Arc<Mutex<Option<(Vec<f64>, bool)>>>;
```

with:

```rust
/// Attack / release ramp (s), to avoid clicks.
const RAMP_SECS: f64 = 0.012;

// FM electric-piano voice parameters (carrier oscillates at the fundamental).
/// Modulator-to-carrier frequency ratio.
const EP_MOD_RATIO: f64 = 1.0;
/// Peak FM modulation index (attack brightness).
const EP_INDEX: f64 = 3.0;
/// Time constant (s) of the modulation-index decay — the bright "tine" attack.
const EP_MOD_DECAY: f64 = 0.18;
/// Time constant (s) of the amplitude decay — the percussive body.
const EP_AMP_DECAY: f64 = 0.9;

/// Reference-tone timbre. Stable indices (persisted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timbre {
    /// FM electric-piano voice (default).
    ElectricPiano,
    /// Pure sine (the original simple tone).
    Sine,
}

impl Timbre {
    pub fn index(self) -> usize {
        match self {
            Timbre::ElectricPiano => 0,
            Timbre::Sine => 1,
        }
    }
    pub fn from_idx(i: usize) -> Self {
        match i {
            1 => Timbre::Sine,
            _ => Timbre::ElectricPiano,
        }
    }
}

/// Shared slot for the next play request: the frequencies, whether to sound them together
/// (block chord) or one after another (arpejo), and the timbre.
type Pending = Arc<Mutex<Option<(Vec<f64>, bool, Timbre)>>>;
```

- [ ] **Step 4: Update `Tone::play` and the audio callback for the timbre**

Replace `Tone::play`:

```rust
    /// Queue a tone. A single frequency is a sustained note; several play together (a block
    /// chord, `NOTE_SECS`) when `together`, or one after another (an arpejo) otherwise.
    /// Returns the total planned playback duration so the caller can gate listening until it
    /// ends. When muted or with no output device, plays nothing and returns `Duration::ZERO`
    /// (listen immediately).
    pub fn play(&self, freqs: &[f64], together: bool, timbre: Timbre) -> Duration {
        if !self.playable || !self.audible.load(Ordering::Relaxed) || freqs.is_empty() {
            return Duration::ZERO;
        }
        let secs = if freqs.len() == 1 || together {
            NOTE_SECS
        } else {
            ARP_SECS * freqs.len() as f64
        };
        *self.pending.lock().unwrap() = Some((freqs.to_vec(), together, timbre));
        Duration::from_secs_f64(secs)
    }
```

In `build`, update the callback's `slot.take()` destructure:

```rust
            if let Ok(mut slot) = pending.try_lock()
                && let Some((freqs, together, timbre)) = slot.take()
            {
                synth.load(&freqs, together, timbre);
            }
```

- [ ] **Step 5: Extend `ToneSynth` (second phase per voice + timbre + FM render)**

Replace the whole `ToneSynth` struct, `new`, `load`, and `next` (the `Seg` struct stays as-is):

```rust
/// Renders a queue of enveloped (poly)phonic segments, one sample at a time. The active
/// segment is flattened into `cur_freqs` + `phases` (+ `mod_phases` for FM) so a sample can
/// read a frequency and advance its phases without a borrow conflict.
struct ToneSynth {
    sr: f64,
    queue: VecDeque<Seg>,
    cur_freqs: Vec<f64>,
    phases: Vec<f64>,
    mod_phases: Vec<f64>,
    timbre: Timbre,
    left: usize,
    total: usize,
}

impl ToneSynth {
    fn new(sr: f64) -> Self {
        ToneSynth {
            sr,
            queue: VecDeque::new(),
            cur_freqs: Vec::new(),
            phases: Vec::new(),
            mod_phases: Vec::new(),
            timbre: Timbre::ElectricPiano,
            left: 0,
            total: 0,
        }
    }

    /// Replace the queue with new segments (clears anything still playing). `together` sounds
    /// all freqs as one block-chord segment; otherwise each freq is its own arpejo segment.
    fn load(&mut self, freqs: &[f64], together: bool, timbre: Timbre) {
        self.queue.clear();
        self.cur_freqs.clear();
        self.phases.clear();
        self.mod_phases.clear();
        self.timbre = timbre;
        self.left = 0;
        self.total = 0;
        if freqs.len() <= 1 || together {
            let total = (self.sr * NOTE_SECS).max(1.0) as usize;
            self.queue.push_back(Seg {
                freqs: freqs.to_vec(),
                total,
            });
        } else {
            let total = (self.sr * ARP_SECS).max(1.0) as usize;
            for &freq in freqs {
                self.queue.push_back(Seg {
                    freqs: vec![freq],
                    total,
                });
            }
        }
    }

    fn next(&mut self) -> f32 {
        if self.left == 0 {
            match self.queue.pop_front() {
                Some(seg) => {
                    self.total = seg.total;
                    self.left = seg.total;
                    self.phases = vec![0.0; seg.freqs.len()];
                    self.mod_phases = vec![0.0; seg.freqs.len()];
                    self.cur_freqs = seg.freqs;
                }
                None => {
                    self.cur_freqs.clear();
                    return 0.0;
                }
            }
        }
        if self.cur_freqs.is_empty() {
            return 0.0;
        }
        let pos = self.total - self.left;
        let t = pos as f64 / self.sr; // seconds since the strike
        let ramp = ((RAMP_SECS * self.sr) as usize).max(1);
        let ramp_env = if pos < ramp {
            pos as f32 / ramp as f32
        } else if self.left < ramp {
            self.left as f32 / ramp as f32
        } else {
            1.0
        };
        let n = self.cur_freqs.len();
        let mut sample = 0.0f32;
        match self.timbre {
            Timbre::Sine => {
                for i in 0..n {
                    // Copy the freq out first so the immutable borrow ends before `phases[i]`
                    // is mutated (the disjoint-access reason this pattern compiles).
                    let freq = self.cur_freqs[i];
                    self.phases[i] += std::f64::consts::TAU * freq / self.sr;
                    sample += self.phases[i].sin() as f32;
                }
                sample = (sample / n as f32) * ramp_env;
            }
            Timbre::ElectricPiano => {
                let index = EP_INDEX * (-t / EP_MOD_DECAY).exp();
                let amp = (-t / EP_AMP_DECAY).exp() as f32;
                for i in 0..n {
                    let freq = self.cur_freqs[i];
                    self.phases[i] += std::f64::consts::TAU * freq / self.sr;
                    self.mod_phases[i] += std::f64::consts::TAU * freq * EP_MOD_RATIO / self.sr;
                    let s = (self.phases[i] + index * self.mod_phases[i].sin()).sin();
                    sample += s as f32;
                }
                sample = (sample / n as f32) * ramp_env * amp;
            }
        }
        self.left -= 1;
        sample * GAIN
    }
}
```

- [ ] **Step 6: Add `timbre_idx` to `config.rs`**

In `crates/vocalize/src/config.rs`, add the field after `play_style_idx`:

```rust
    /// Chord playback style index (0 = together); see [`crate::exercise::PlayStyle`].
    #[serde(default)]
    pub play_style_idx: usize,
    /// Reference-tone timbre index (0 = electric piano); see [`crate::tone::Timbre`].
    #[serde(default)]
    pub timbre_idx: usize,
```

Add it to `Default` (after `play_style_idx: 0,`):

```rust
            play_style_idx: 0,
            timbre_idx: 0,
```

In the `roundtrips_json` test, add `timbre_idx: 1,` to the constructed struct and assert it:

```rust
            mode_idx: 2,
            play_style_idx: 1,
            timbre_idx: 1,
            cents_window: 25.0,
            sustain_ms: 800,
        };
```
```rust
        assert_eq!(loaded.play_style_idx, 1);
        assert_eq!(loaded.timbre_idx, 1);
```

In `missing_fields_use_defaults`, add:

```rust
        assert_eq!(cfg.play_style_idx, 0);
        assert_eq!(cfg.timbre_idx, 0);
```

- [ ] **Step 7: Wire the timbre through `main.rs`**

In `crates/vocalize/src/main.rs`, add the import next to the other `use` lines (after `use config::VocalizeConfig;`):

```rust
use tone::Timbre;
```

Add the field to `State` (after the `play_style` field):

```rust
    /// Whether chords play together (block) or as an arpejo.
    play_style: PlayStyle,
    /// Reference-tone timbre (electric piano or sine).
    timbre: Timbre,
    cents_window: f64,
```

In `Default`, build it and pass it to the first `play` (replace the `play_style` line + the `present` block):

```rust
        let play_style = PlayStyle::from_idx(cfg.play_style_idx);
        let timbre = Timbre::from_idx(cfg.timbre_idx);
        let cents_window = cfg.cents_window;
        let sustain_ms = cfg.sustain_ms as f64;
        let mut rng = seed();
        let degree = next_degree(&mut rng, scale.degree_count(), usize::MAX);
        let item = exercise::item_at(&scale, mode, degree);
        let matcher = Matcher::new(&item, cents_window, sustain_ms);
        let tone = tone::Tone::new(cfg.audible);
        let present = if cfg.enabled {
            tone.play(&freqs_of(&item), play_style == PlayStyle::Together, timbre)
        } else {
            std::time::Duration::ZERO
        };
```

And add `timbre,` to the returned `State { … }` literal (after `play_style,`):

```rust
            scale,
            mode,
            play_style,
            timbre,
            cents_window,
```

In `advance()`, pass the timbre:

```rust
        let present = if self.enabled {
            self.tone.play(
                &freqs_of(&self.item),
                self.play_style == PlayStyle::Together,
                self.timbre,
            )
        } else {
            Duration::ZERO
        };
```

In the `Message::Replay if self.enabled` arm, pass the timbre:

```rust
            Message::Replay if self.enabled => {
                let present = self.tone.play(
                    &freqs_of(&self.item),
                    self.play_style == PlayStyle::Together,
                    self.timbre,
                );
                self.present_until = Some(Instant::now() + present);
            }
```

In `persist()`, write `timbre_idx` (after `play_style_idx`):

```rust
                play_style_idx: self.play_style.index(),
                timbre_idx: self.timbre.index(),
                cents_window: self.cents_window,
```

- [ ] **Step 8: Run the tests + clippy**

Run: `cargo +stable test -p vocalize`
Expected: PASS — the 3 new tone tests + updated config tests + all prior `vocalize` tests (21 total: 18 prior + `timbre_idx_roundtrips` + `output_is_finite_and_bounded` + `electric_piano_decays`).
Run: `cargo +stable fmt -p vocalize -- --check` (run `cargo +stable fmt -p vocalize` to fix)
Run: `cargo +stable clippy -p vocalize --all-targets -- -D warnings`
Expected: **no warnings** — `Timbre::index`/`from_idx`, the new fields, and the EP constants are all consumed (the tray selector and `label`/`ALL` come in Task 2; they are not referenced yet, so nothing is dead).

- [ ] **Step 9: Commit**

```bash
git add crates/vocalize/src/tone.rs crates/vocalize/src/config.rs crates/vocalize/src/main.rs
git commit -m "vocalize: FM electric-piano timbre (default) + Sine; selectable wiring"
```

---

### Task 2: Tray "Timbre" submenu + `SetTimbre` message

Expose the timbre from the tray. Lands the consumers for `Timbre::label`/`ALL`.

**Files:**
- Modify: `crates/vocalize/src/tone.rs` (add `label` + `ALL`)
- Modify: `crates/vocalize/src/main.rs` (message + arm)
- Modify: `crates/vocalize/src/tray.rs`

**Interfaces:**
- Consumes: `tone::Timbre` (Task 1), the existing tray submenu/RadioGroup idiom.
- Produces: `Message::SetTimbre(tone::Timbre)`; `VocalizeTray.timbre`.

- [ ] **Step 1: Add `label` + `ALL` to `Timbre`**

In `crates/vocalize/src/tone.rs`, inside `impl Timbre` (after `from_idx`):

```rust
    pub fn label(self) -> &'static str {
        match self {
            Timbre::ElectricPiano => "Piano elétrico",
            Timbre::Sine => "Senóide",
        }
    }
    pub const ALL: [Timbre; 2] = [Timbre::ElectricPiano, Timbre::Sine];
```

- [ ] **Step 2: Add the `SetTimbre` message + handler in `main.rs`**

Add the variant to `enum Message` (after `SetPlayStyle`):

```rust
    SetPlayStyle(exercise::PlayStyle),
    SetTimbre(tone::Timbre),
    SetCents(f64),
```

Add the `update` arm (after the `Message::SetPlayStyle` arm, before `Message::SetCents`):

```rust
            Message::SetTimbre(t) => {
                self.timbre = t;
                self.persist();
                self.reset();
            }
```

- [ ] **Step 3: Add the "Timbre" submenu to `tray.rs`**

In `crates/vocalize/src/tray.rs`, update the import line:

```rust
use crate::exercise::{Mode, PlayStyle, ScaleKind};
use crate::tone::Timbre;
use crate::{CENTS_STEPS, Message, ROOTS, SUSTAIN_STEPS, exercise};
```

Add the field to `VocalizeTray` (after `play_style`):

```rust
    pub play_style: PlayStyle,
    pub timbre: Timbre,
    pub cents_window: f64,
```

Add the selected-index local (after `let play_style_idx = self.play_style.index();`):

```rust
        let timbre_idx = self.timbre.index();
```

Insert a "Timbre" `SubMenu` immediately AFTER the "Reprodução" submenu (i.e. right before the `SubMenu { label: "Tolerância".into(), … }`):

```rust
            SubMenu {
                label: "Timbre".into(),
                submenu: vec![
                    RadioGroup {
                        selected: timbre_idx,
                        select: Box::new(|this: &mut Self, idx| {
                            let t = Timbre::from_idx(idx);
                            this.timbre = t;
                            let _ = this.tx.unbounded_send(Message::SetTimbre(t));
                        }),
                        options: Timbre::ALL
                            .iter()
                            .map(|t| RadioItem {
                                label: t.label().into(),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
```

- [ ] **Step 4: Pass the timbre to the tray constructor in `main.rs`**

In `event_stream`, in the `tray::VocalizeTray { … }` literal, add (after `play_style: …`):

```rust
        play_style: PlayStyle::from_idx(cfg.play_style_idx),
        timbre: Timbre::from_idx(cfg.timbre_idx),
        cents_window: cfg.cents_window,
```

- [ ] **Step 5: Full gate**

Run: `cargo +stable fmt --all -- --check` (run `cargo +stable fmt --all` to fix)
Run: `cargo +stable clippy --all-targets -- -D warnings`
Expected: **no warnings anywhere** — `Timbre::label`/`ALL` are now consumed by the tray.
Run: `cargo +stable test --workspace`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/vocalize/src/tone.rs crates/vocalize/src/main.rs crates/vocalize/src/tray.rs
git commit -m "vocalize: tray Timbre submenu (Piano elétrico / Senóide)"
```

---

### Task 3: Docs

**Files:**
- Modify: `CLAUDE.md`
- Modify: `README.md`

- [ ] **Step 1: Update the `vocalize` bullet in `CLAUDE.md`**

In the `vocalize` bullet, extend the `tone.rs` description to mention the timbre and add `timbre_idx` to the listed config fields. Specifically:
- In the sentence describing `tone.rs`, after "sums N simultaneous sine voices", add that the voice is one of two timbres — an FM **electric piano** (`Timbre::ElectricPiano`, default: carrier + a fast-decaying modulator and a percussive amplitude envelope) or a pure **`Sine`** — selected by the tray **Timbre** submenu.
- In the `VocalizeConfig { … }` field list, insert `timbre_idx` after `play_style_idx`.
- In the persisted tray-choices list at the end of the bullet, add `Timbre` after `Reprodução`.

- [ ] **Step 2: Update the `vocalize` bullet in `README.md`**

In the `vocalize` user-facing bullet (the one listing adjustable settings), add the timbre to the tray-adjustables, e.g. extend "…chord playback (together or arpeggio), tolerance, and sustain…" to also mention "tone timbre (electric piano or sine)". Match the existing wording/format.

- [ ] **Step 3: Verify gate + commit**

Run: `cargo +stable fmt --all -- --check && cargo +stable clippy --all-targets -- -D warnings && cargo +stable test --workspace`
Expected: all clean/PASS (docs-only change).

```bash
git add CLAUDE.md README.md
git commit -m "docs: document the vocalize electric-piano timbre"
```

---

## Self-Review

**Spec coverage:**
- FM electric-piano voice + percussive envelope → Task 1 (`ToneSynth` EP branch, `EP_*` constants). ✓
- `Timbre {ElectricPiano, Sine}` in `tone.rs`, EP default → Task 1. ✓
- `Sine` reproduces current sound → Task 1 (`Timbre::Sine` branch = the prior summed-sine + ramp). ✓
- `Tone::play` + `Pending` carry timbre → Task 1. ✓
- config `timbre_idx` (default 0) → Task 1. ✓
- main wiring (State/advance/Replay/Default/persist) → Task 1. ✓
- tray "Timbre" submenu + `SetTimbre` → Task 2. ✓
- Tests: Timbre roundtrip, bounded output, EP decays; config roundtrip → Task 1. ✓
- Docs → Task 3. ✓
- Backward-compat (default EP) → Task 1 (`timbre_idx` default 0). ✓
- Out-of-scope items not implemented. ✓

**Placeholder scan:** none — every code step shows complete code; the only prose-described edits are the docs in Task 3 (which reference exact insertion points).

**Type consistency:** `Timbre::{index,from_idx,label,ALL}`, `Tone::play(freqs, together, timbre)`, `ToneSynth::load(freqs, together, timbre)`, `Pending = Option<(Vec<f64>, bool, Timbre)>`, `config.timbre_idx`, `State.timbre`, `Message::SetTimbre` are used identically across tasks. `EP_MOD_RATIO`/`EP_INDEX`/`EP_MOD_DECAY`/`EP_AMP_DECAY` defined and used in Task 1's `next()`.
