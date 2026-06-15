//! Shared microphone pitch detection + note math for the sceno overlay apps.

pub mod capture;
pub mod color;
pub mod detect;
pub mod note;
pub mod smooth;

pub use capture::{MIN_CLARITY, WINDOW, run_capture};
pub use color::cents_color;
pub use detect::detect_frequency;
pub use note::{A4, Note, frequency_to_note, is_in_tune, midi_name, note_to_frequency};
pub use smooth::Smoother;
