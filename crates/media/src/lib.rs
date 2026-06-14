//! Now-playing tracking + lyrics sources shared by the sceno overlay apps.

pub mod cue;
pub mod library;
pub mod lrc;
pub mod lrclib;
pub mod player;
pub mod sync;
pub mod ultrastar;

pub use cue::{CueEntry, cue_at};
pub use library::{LibraryEntry, Song};
pub use lrclib::TrackQuery;
pub use player::PlayerEvent;
pub use sync::TimelineSync;
pub use ultrastar::{NoteEvent, UltraStarSong};
