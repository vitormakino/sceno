//! Microphone capture + pitch detection feeding Message::PitchUpdate.

use pitch_detection::detector::mcleod::McLeodDetector;
use pitch_detection::detector::PitchDetector;

/// Estimate the fundamental frequency of a mono f32 buffer, or `None` if no
/// clear pitch. `sample_rate` in Hz; `min_clarity` in 0..1.
pub fn detect_frequency(samples: &[f32], sample_rate: u32, min_clarity: f64) -> Option<f64> {
    let size = samples.len();
    if size < 256 {
        return None;
    }
    let padding = size / 2;
    let signal: Vec<f64> = samples.iter().map(|&s| s as f64).collect();
    let mut detector = McLeodDetector::new(size, padding);
    detector
        .get_pitch(&signal, sample_rate as usize, 0.15, min_clarity)
        .map(|p| p.frequency)
}

// ── Capture runner ────────────────────────────────────────────────────────────

pub use capture_impl::run;

mod capture_impl {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use futures::channel::mpsc::UnboundedSender;
    use std::sync::{Arc, Mutex};

    use crate::Message;

    const A4: f64 = 440.0;
    const WINDOW: usize = 4096;
    const MIN_CLARITY: f64 = 0.6;

    /// Owns the cpal input stream + analysis loop; sends PitchUpdate until the app exits.
    pub fn run(tx: UnboundedSender<Message>) {
        let host = cpal::default_host();
        let Some(device) = host.default_input_device() else {
            eprintln!("[tuner] no input device");
            return;
        };
        let Ok(cfg) = device.default_input_config() else {
            eprintln!("[tuner] no input config");
            return;
        };
        let sample_rate = cfg.sample_rate().0;
        let channels = cfg.channels() as usize;
        let sample_format = cfg.sample_format();

        let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(WINDOW)));
        let cb_buf = buf.clone();

        let err_fn = |e: cpal::StreamError| eprintln!("[tuner] stream error: {e}");

        // Build the stream for the sample format.  The common case is f32; for
        // other formats we convert to f32 before pushing into the ring buffer.
        use cpal::SampleFormat;
        let stream = match sample_format {
            SampleFormat::F32 => {
                let b = cb_buf.clone();
                device.build_input_stream(
                    &cfg.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let mut buf = b.lock().unwrap();
                        for frame in data.chunks(channels) {
                            buf.push(frame[0]);
                        }
                        let len = buf.len();
                        if len > WINDOW {
                            buf.drain(0..len - WINDOW);
                        }
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::I16 => {
                let b = cb_buf.clone();
                device.build_input_stream(
                    &cfg.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let mut buf = b.lock().unwrap();
                        for frame in data.chunks(channels) {
                            buf.push(frame[0] as f32 / i16::MAX as f32);
                        }
                        let len = buf.len();
                        if len > WINDOW {
                            buf.drain(0..len - WINDOW);
                        }
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::U16 => {
                let b = cb_buf.clone();
                device.build_input_stream(
                    &cfg.into(),
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        let mut buf = b.lock().unwrap();
                        for frame in data.chunks(channels) {
                            buf.push((frame[0] as f32 / u16::MAX as f32) * 2.0 - 1.0);
                        }
                        let len = buf.len();
                        if len > WINDOW {
                            buf.drain(0..len - WINDOW);
                        }
                    },
                    err_fn,
                    None,
                )
            }
            other => {
                eprintln!("[tuner] unsupported sample format: {other:?}");
                return;
            }
        };

        let Ok(stream) = stream else {
            eprintln!("[tuner] build_input_stream failed");
            return;
        };
        if stream.play().is_err() {
            eprintln!("[tuner] stream.play failed");
            return;
        }

        loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let window: Vec<f32> = { buf.lock().unwrap().clone() };
            let note = (window.len() >= WINDOW)
                .then(|| super::detect_frequency(&window, sample_rate, MIN_CLARITY))
                .flatten()
                .map(|f| crate::note::frequency_to_note(f, A4));
            if tx.unbounded_send(Message::PitchUpdate(note)).is_err() {
                break;
            }
        }
        drop(stream); // keep stream alive until loop ends
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine(freq: f32, sr: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f32 / sr as f32).sin())
            .collect()
    }

    #[test]
    fn detects_a440_sine() {
        let sr = 44_100;
        let buf = sine(440.0, sr, 4096);
        let f = detect_frequency(&buf, sr, 0.5).expect("should detect a clear sine");
        assert!((f - 440.0).abs() < 5.0, "got {f}");
    }

    #[test]
    fn silence_has_no_pitch() {
        let buf = vec![0.0f32; 4096];
        assert!(detect_frequency(&buf, 44_100, 0.5).is_none());
    }
}
