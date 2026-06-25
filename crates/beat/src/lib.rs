//! Beat-clock timing, metronome click synthesis, and best-effort tempo detection.
//!
//! - [`clock`] — a thread-shared [`SharedClock`] (tempo + downbeat phase) that is
//!   the single source of truth read by both the audio clicker and the UI flash.
//! - [`click`] — a cpal output stream that renders sample-accurate metronome
//!   clicks (accented downbeat) off the shared clock.
//! - [`detect`] — best-effort tempo estimation from the system-audio monitor.

pub mod click;
pub mod clock;
pub mod detect;

pub use click::run_click;
pub use clock::{MAX_BPM, MIN_BPM, SharedClock, beat_position, tap_bpm};
pub use detect::{BpmEstimate, run_detect};
