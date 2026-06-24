//! On-demand reference-tone playback via a cpal output stream.
//!
//! A dedicated thread owns the output stream (like `beat::click`) and renders an
//! enveloped sine. The UI pushes a play request (a slice of frequencies) through a
//! shared slot read once per audio buffer; one frequency plays as a sustained tone,
//! several play as a short ascending arpejo so each chord pitch is heard in turn.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Duration of a single sustained note (s).
const NOTE_SECS: f64 = 1.0;
/// Duration of each note within a chord arpejo (s).
const ARP_SECS: f64 = 0.35;
const GAIN: f32 = 0.25;
/// Attack / release ramp (s), to avoid clicks.
const RAMP_SECS: f64 = 0.012;

/// Handle to the tone player. Cheap to clone (two `Arc`s).
#[derive(Clone)]
pub struct Tone {
    pending: Arc<Mutex<Option<Vec<f64>>>>,
    audible: Arc<AtomicBool>,
}

impl Tone {
    /// Spawn the output thread and return a handle. Degrades to a silent handle if
    /// no output device is available (requests are simply never rendered).
    pub fn new(audible: bool) -> Tone {
        let pending: Arc<Mutex<Option<Vec<f64>>>> = Arc::new(Mutex::new(None));
        let p = pending.clone();
        std::thread::spawn(move || run_output(p));
        Tone {
            pending,
            audible: Arc::new(AtomicBool::new(audible)),
        }
    }

    pub fn set_audible(&self, on: bool) {
        self.audible.store(on, Ordering::Relaxed);
    }

    /// Queue a tone (one freq = sustained note; many = arpejo). Returns the total
    /// planned playback duration so the caller can gate listening until it ends.
    /// When muted, plays nothing and returns `Duration::ZERO` (listen immediately).
    pub fn play(&self, freqs: &[f64]) -> Duration {
        if !self.audible.load(Ordering::Relaxed) || freqs.is_empty() {
            return Duration::ZERO;
        }
        let secs = if freqs.len() == 1 {
            NOTE_SECS
        } else {
            ARP_SECS * freqs.len() as f64
        };
        *self.pending.lock().unwrap() = Some(freqs.to_vec());
        Duration::from_secs_f64(secs)
    }
}

fn run_output(pending: Arc<Mutex<Option<Vec<f64>>>>) {
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        eprintln!("[vocalize] no output device");
        return;
    };
    let Ok(cfg) = device.default_output_config() else {
        eprintln!("[vocalize] no output config");
        return;
    };
    overlay::debug(
        "vocalize",
        format_args!(
            "output: {} @ {} Hz, {} ch, {:?}",
            device.name().unwrap_or_else(|_| "?".into()),
            cfg.sample_rate().0,
            cfg.channels(),
            cfg.sample_format()
        ),
    );
    let sr = cfg.sample_rate().0 as f64;
    let channels = cfg.channels() as usize;
    let err_fn = |e: cpal::StreamError| eprintln!("[vocalize] stream error: {e}");
    use cpal::SampleFormat;
    let config = cfg.config();
    let stream = match cfg.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, pending, sr, channels, err_fn),
        SampleFormat::I16 => build::<i16>(&device, &config, pending, sr, channels, err_fn),
        SampleFormat::U16 => build::<u16>(&device, &config, pending, sr, channels, err_fn),
        other => {
            eprintln!("[vocalize] unsupported output sample format: {other:?}");
            return;
        }
    };
    let Ok(stream) = stream else {
        eprintln!("[vocalize] build_output_stream failed");
        return;
    };
    if stream.play().is_err() {
        eprintln!("[vocalize] stream.play failed");
        return;
    }
    // Keep the stream alive for the process lifetime (the tray "Sair" exits).
    let _stream = stream;
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    pending: Arc<Mutex<Option<Vec<f64>>>>,
    sr: f64,
    channels: usize,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    let mut synth = ToneSynth::new(sr);
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            if let Ok(mut slot) = pending.try_lock()
                && let Some(freqs) = slot.take()
            {
                synth.load(&freqs);
            }
            for frame in data.chunks_mut(channels) {
                let v = T::from_sample(synth.next());
                for ch in frame.iter_mut() {
                    *ch = v;
                }
            }
        },
        err_fn,
        None,
    )
}

struct Seg {
    freq: f64,
    total: usize,
}

/// Renders a queue of enveloped sine segments, one sample at a time.
struct ToneSynth {
    sr: f64,
    queue: VecDeque<Seg>,
    cur: Option<Seg>,
    left: usize,
    total: usize,
    phase: f64,
}

impl ToneSynth {
    fn new(sr: f64) -> Self {
        ToneSynth {
            sr,
            queue: VecDeque::new(),
            cur: None,
            left: 0,
            total: 0,
            phase: 0.0,
        }
    }

    /// Replace the queue with new segments (clears anything still playing).
    fn load(&mut self, freqs: &[f64]) {
        self.queue.clear();
        self.cur = None;
        self.left = 0;
        self.total = 0;
        self.phase = 0.0;
        let secs = if freqs.len() == 1 {
            NOTE_SECS
        } else {
            ARP_SECS
        };
        let total = (self.sr * secs) as usize;
        for &freq in freqs {
            self.queue.push_back(Seg { freq, total });
        }
    }

    fn next(&mut self) -> f32 {
        if self.left == 0 {
            match self.queue.pop_front() {
                Some(seg) => {
                    self.total = seg.total;
                    self.left = seg.total;
                    self.phase = 0.0;
                    self.cur = Some(seg);
                }
                None => {
                    self.cur = None;
                    return 0.0;
                }
            }
        }
        let Some(seg) = &self.cur else {
            return 0.0;
        };
        let pos = self.total - self.left;
        let ramp = ((RAMP_SECS * self.sr) as usize).max(1);
        let env = if pos < ramp {
            pos as f32 / ramp as f32
        } else if self.left < ramp {
            self.left as f32 / ramp as f32
        } else {
            1.0
        };
        self.phase += std::f64::consts::TAU * seg.freq / self.sr;
        self.left -= 1;
        (self.phase.sin() as f32) * env * GAIN
    }
}
