//! Microphone capture shim: feeds `pitch`-detected notes into `Message::PitchUpdate`.

use futures::channel::mpsc::UnboundedSender;

use crate::Message;

/// Owns the cpal input stream + analysis loop (via [`pitch::run_capture`]);
/// sends `PitchUpdate` until the app exits (the receiver is dropped).
pub fn run(tx: UnboundedSender<Message>) {
    pitch::run_capture(|note| tx.unbounded_send(Message::PitchUpdate(note)).is_ok());
}
