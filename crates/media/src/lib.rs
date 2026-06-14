//! Now-playing tracking + lyrics sources shared by the sceno overlay apps.

pub mod cue;
pub mod lrc;
pub mod lrclib;
pub mod player;
pub mod sync;

pub use cue::{CueEntry, cue_at};
pub use lrclib::TrackQuery;
pub use player::PlayerEvent;
pub use sync::TimelineSync;
