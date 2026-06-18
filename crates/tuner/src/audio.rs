//! Microphone capture shim: feeds the `pitch`-detected frequency into
//! `Message::PitchUpdate`; the app maps it to a note with its chosen reference
//! pitch and instrument preset.

use futures::channel::mpsc::UnboundedSender;

use crate::Message;

/// Owns the cpal input stream + analysis loop (via [`pitch::run_capture`]);
/// sends `PitchUpdate` until the app exits (the receiver is dropped).
pub fn run(tx: UnboundedSender<Message>) {
    pitch::run_capture(|freq| tx.unbounded_send(Message::PitchUpdate(freq)).is_ok());
}
