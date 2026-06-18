# Design: `lyrics` word-by-word highlight + lookahead

**Date:** 2026-06-18
**Status:** spec (brainstorming → spec)

## Context

The `lyrics` app shows one synced caption line at a time: `apply_timeline_caption`
picks the active `CueEntry` via `cue_at(&cues, t)` and the `view` renders it as a
single `text()` in a translucent pill. This is the line-level experience every basic
lyrics tool ships.

The features that make Apple Music and Musixmatch *feel* premium are **word-by-word
highlighting** (the line fills in as it is sung) and a **lookahead** line (the next
line shown dimmed so the singer can anticipate). For `sceno` these are unusually cheap:

- **The data path already exists.** LRCLIB serves *enhanced* LRC with per-word tags
  (`<mm:ss.xx>word`). `media/src/lrc.rs` already parses those tags — and then
  **throws them away** in `strip_word_tags`. We are discarding the exact timing we
  need.
- **Sync comes for free.** Word times are absolute LRC times; the highlight is driven
  by the same `t = sync.current_time() + offset` already computed in
  `apply_timeline_caption`, so the per-song sync nudge applies to word highlighting
  with no extra work.
- **The tick is already there.** `timeline_tick_stream` fires every 100 ms while
  playing — fine granularity for advancing the highlight.

This is the highest-ROI item from the competitive analysis: it turns the signature
"karaoke fill" effect into "stop discarding data we already parse".

## Goals

- Preserve per-word timings from enhanced LRC into the shared cue model.
- Render the active line with sung words bright and not-yet-sung words dim, advancing
  with playback.
- Render the next upcoming line dimmed below the active one (lookahead).
- Degrade gracefully: line-only LRC (no `<>` tags) renders exactly as today
  (whole line bright), and lookahead is the next line bright-less.

## Non-goals

- No change to LRCLIB fetch/store, MPRIS, or the offset/announcement logic.
- No karaoke-style scoring (separate `karaoke` work item).
- No translation/romanization (separate, later item).
- No re-flow of the auto-stacking strip system.

## Decisions (locked)

| Decision | Choice |
|----------|--------|
| Where word timings live | New `words: Vec<WordTiming>` field on `CueEntry` (in `media`) |
| Word timing meaning | Absolute LRC time of the word's onset, same scale as `start` |
| Fallback when no word tags | `words` is empty → render whole line bright (current behavior) |
| Highlight rule | A word is "sung" once `t >= word.start`; sung = white, unsung = dim |
| Lookahead | Show the next non-empty cue, dimmed, as a smaller second line |
| Surface height | Keep the 80px strip; lookahead is a smaller dim line, not a full 2nd line |
| Toggle | One persisted tray toggle **"Próxima linha"** (lookahead on by default). Word highlight has no toggle — it is automatic and self-degrading |
| Offset interaction | Word highlight uses the same offset-adjusted `t`; nothing new needed |

### Why a field on `CueEntry` (not a parallel structure)

`CueEntry` is the source-agnostic model every lyrics source produces (LRC today,
UltraStar/cue elsewhere). Adding an optional `words` keeps the timeline a single value
and lets non-LRC sources leave it empty. `text` stays as the joined line so all
existing consumers (`cue_at`, tests, the announcement path) are untouched.

## Data model (in `media`)

```rust
// cue.rs
/// Onset of one word within a line, in the same time scale as CueEntry::start.
#[derive(Debug, Clone)]
pub struct WordTiming {
    pub start: f64,
    pub text: String, // includes its trailing space if any, so joining reproduces the line
}

pub struct CueEntry {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<WordTiming>, // empty when the source has no per-word timing
}

impl CueEntry {
    /// How many leading words have been reached at time `t` (0..=words.len()).
    /// Returns 0 when there are no word timings (caller renders the whole line).
    pub fn sung_words(&self, t: f64) -> usize {
        self.words.iter().take_while(|w| t >= w.start).count()
    }
}
```

A lookahead helper replaces the bare `cue_at` lookup in the app:

```rust
/// The active line at `t` and the next non-empty line after it (for lookahead).
pub struct ActiveLines<'a> {
    pub current: Option<&'a CueEntry>,
    pub next: Option<&'a CueEntry>,
}

pub fn lines_at(cues: &[CueEntry], t: f64) -> ActiveLines<'_>;
```

`cue_at` stays (used by tests / the simple path); `lines_at` is the richer lookup.
`next` skips empty-text cues (instrumental-gap markers) so lookahead shows real lyrics,
not a blank.

## Work items

Each numbered item is its own commit, in order.

### 1. Preserve word timings in the parser

`crates/media/src/cue.rs`: add `WordTiming` + the `words` field + `sung_words`.
Update every `CueEntry { .. }` literal in the workspace to set `words` (mostly
`words: Vec::new()` in tests and the UltraStar/cue paths).

`crates/media/src/lrc.rs`: replace `strip_word_tags` with a parser that returns both
the joined text **and** the `Vec<WordTiming>`:

- Walk the line tracking `<mm:ss.xx>` tags. Text following a tag (until the next tag
  or end) is that word's `text`; its `start` is the tag time.
- Text *before* the first word tag (rare, but seen) attaches to a synthetic first word
  whose `start` = the line's own `[..]` timestamp.
- When a line has no `<>` tags, `words` is empty and `text` is the plain line (today's
  behavior, byte-for-byte).
- The line `end` still bounds the last word.

Keep `parse_time` shared between line and word tags.

### 2. Lookahead lookup

`crates/media/src/cue.rs`: add `ActiveLines` + `lines_at`. Unit-test current/next
selection, empty-cue skipping, and out-of-range (`None`).

### 3. Render word highlight + lookahead in the app

`crates/lyrics/src/main.rs`:

- Replace the single `caption: String` with enough state to render words. Two options;
  **chosen:** keep `caption` for the announcement/fallback path, and add the
  offset-adjusted `t` + a borrowed view over `cues` so `view` can compute the
  per-word split. Concretely, store the active cue index + `next` index + `t` updated
  in `apply_timeline_caption`, and have `view` read `state.cues` by index. (This keeps
  the existing announcement/offset/empty logic intact; the announcement is still a
  plain `caption` line with no words.)
- `view`:
  - Build the active line as a `row` of per-word `text` widgets: words `0..sung` in
    `Color::WHITE`, the rest at a dim white (`Color::from_rgba(1,1,1,0.45)`). With no
    word timings, render the whole line as a single white `text` (current look).
  - Below it, when lookahead is enabled and a `next` line exists, a smaller dim
    `text(next.text)` (size `~0.6×`, alpha `~0.5`).
  - Keep both inside the existing pill `container`; stack with `column!`.
  - The `⏱ ±NNN ms` offset chip stays on the active line's row.
- Geometry: the active line + a small dim lookahead line must fit the 80px strip.
  Render lookahead at the reduced size above; if visual testing shows clipping at the
  Large font size, the fallback is to drop lookahead for Large only (no surface
  resize in this spec).

### 4. Tray toggle for lookahead

`crates/lyrics/src/main.rs` + `config.rs`: add `show_next: bool` (default `true`) to
`SavedConfig`, a `CheckmarkItem` **"Próxima linha"** in the tray, a
`Message::SetShowNext(bool)`, persistence, and gate the lookahead render on it.

## Testing

Pure/deterministic (no hardware, no network):

- **Parser (`lrc.rs`):**
  - `[00:01.00]<00:01.00>hello <00:01.50>world` → one cue, `text == "hello world"`,
    `words == [(1.0,"hello "),(1.5,"world")]`.
  - Plain line (no `<>`) → `words` empty, `text` unchanged (extend the existing
    `strips_word_level_tags` test into a `parses_word_level_tags` one; keep a
    plain-line case asserting empty `words`).
  - Leading text before first tag attaches to the line start.
- **`sung_words`:** before first word → 0; mid-line → count of reached words; after
  last → `words.len()`; empty `words` → 0 at any `t`.
- **`lines_at`:** current+next selection, `next` skips empty-text cues, `None` outside
  range / empty list.
- **App:** existing `apply_timeline_caption` tests stay green. Add tests that the
  active/next indices and offset-adjusted `t` are what a nudged song produces (reuse
  the `paused_sync` helper). `view` is not unit-tested (iced render), consistent with
  the current suite.

## Verification

- `cargo +stable fmt --all --check`, `cargo +stable clippy --all-targets -- -D warnings`,
  `cargo +stable test --workspace` all green.
- Manual: play a track with enhanced LRC on LRCLIB (most popular songs have it); the
  active line fills word-by-word in time; the next line shows dim below; a sync nudge
  shifts both the line *and* the word fill together. A song with only line-level LRC
  still shows the whole line bright (no regression). Toggling **"Próxima linha"** off
  hides the lookahead.

## Out of scope (tracked, separate items)

- Translation / romanization second line (Musixmatch-style).
- Karaoke scoring / gamification (`karaoke`).
- Bumping the strip height or a dedicated tall lyrics panel.
- Smooth per-letter/gradient fill (we highlight at word granularity).
