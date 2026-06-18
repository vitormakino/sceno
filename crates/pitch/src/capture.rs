//! Microphone capture + analysis loop, reusable across apps.
//!
//! Owns the cpal input stream and a 50 ms analysis loop, delivering a smoothed
//! fundamental frequency in Hz (or `None`) to a caller-supplied `sink` each tick.
//! Consumers map the frequency to a [`crate::note::Note`] with whatever reference
//! pitch they want. The `sink` returns `false` to stop the loop (e.g. its receiver
//! was dropped because the app is exiting).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

use crate::detect::detect_frequency;
use crate::smooth::Smoother;

/// Analysis window size (samples) — also the minimum buffer fill before detecting.
pub const WINDOW: usize = 4096;
/// McLeod clarity threshold (0..1) below which a frame is treated as no pitch.
pub const MIN_CLARITY: f64 = 0.6;

/// Open the default input device and run the capture/analysis loop, calling
/// `sink` with each smoothed frequency (Hz). Blocks; intended to own a dedicated
/// thread. Returns when `sink` returns `false` or the device/stream can't be set up.
pub fn run_capture(mut sink: impl FnMut(Option<f64>) -> bool) {
    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
        eprintln!("[pitch] no input device");
        return;
    };
    let Ok(cfg) = device.default_input_config() else {
        eprintln!("[pitch] no input config");
        return;
    };
    let sample_rate = cfg.sample_rate().0;
    let channels = cfg.channels() as usize;
    let sample_format = cfg.sample_format();

    // Opt-in stderr tracing (`SCENO_DEBUG=1`): chosen device + detected notes.
    overlay::debug(
        "pitch",
        format_args!(
            "input: {} @ {sample_rate} Hz, {channels} ch, {sample_format:?}",
            device.name().unwrap_or_else(|_| "?".into())
        ),
    );

    let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(WINDOW)));
    let cb_buf = buf.clone();

    let err_fn = |e: cpal::StreamError| eprintln!("[pitch] stream error: {e}");

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
            eprintln!("[pitch] unsupported sample format: {other:?}");
            return;
        }
    };

    let Ok(stream) = stream else {
        eprintln!("[pitch] build_input_stream failed");
        return;
    };
    if stream.play().is_err() {
        eprintln!("[pitch] stream.play failed");
        return;
    }

    let mut smoother = Smoother::default();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let window: Vec<f32> = { buf.lock().unwrap().clone() };
        let raw = (window.len() >= WINDOW)
            .then(|| detect_frequency(&window, sample_rate, MIN_CLARITY))
            .flatten();
        let freq = smoother.update(raw);
        if let Some(f) = freq {
            overlay::debug("pitch", format_args!("{f:.1} Hz"));
        }
        if !sink(freq) {
            break;
        }
    }
    drop(stream); // keep stream alive until loop ends
}
