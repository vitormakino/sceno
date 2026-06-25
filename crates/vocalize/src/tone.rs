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

/// Shared slot for the next play request: the frequencies and whether to sound them
/// together (block chord) or one after another (arpejo).
type Pending = Arc<Mutex<Option<(Vec<f64>, bool)>>>;

/// Handle to the tone player. Cheap to clone (two `Arc`s + a flag).
#[derive(Clone)]
pub struct Tone {
    /// The next play request (see [`Pending`]).
    pending: Pending,
    audible: Arc<AtomicBool>,
    /// Whether an output device was present at startup. When false, [`Tone::play`]
    /// is a no-op returning `Duration::ZERO`, so the caller never gates listening
    /// on a tone that will never sound (e.g. a headless / no-audio session). The
    /// probe is synchronous, so it is settled before the first `play` call — unlike
    /// the output thread, which finishes building the stream a few ms later.
    playable: bool,
}

impl Tone {
    /// Probe for an output device and (if present) spawn the output thread, returning
    /// a handle. With no output device the handle is silent: `play` renders nothing
    /// and reports zero duration.
    pub fn new(audible: bool) -> Tone {
        let playable = cpal::default_host().default_output_device().is_some();
        let pending: Pending = Arc::new(Mutex::new(None));
        let p = pending.clone();
        if playable {
            std::thread::spawn(move || run_output(p));
        }
        Tone {
            pending,
            audible: Arc::new(AtomicBool::new(audible)),
            playable,
        }
    }

    pub fn set_audible(&self, on: bool) {
        self.audible.store(on, Ordering::Relaxed);
    }

    /// Queue a tone. A single frequency is a sustained note; several play together (a block
    /// chord, `NOTE_SECS`) when `together`, or one after another (an arpejo) otherwise.
    /// Returns the total planned playback duration so the caller can gate listening until it
    /// ends. When muted or with no output device, plays nothing and returns `Duration::ZERO`
    /// (listen immediately).
    pub fn play(&self, freqs: &[f64], together: bool) -> Duration {
        if !self.playable || !self.audible.load(Ordering::Relaxed) || freqs.is_empty() {
            return Duration::ZERO;
        }
        let secs = if freqs.len() == 1 || together {
            NOTE_SECS
        } else {
            ARP_SECS * freqs.len() as f64
        };
        *self.pending.lock().unwrap() = Some((freqs.to_vec(), together));
        Duration::from_secs_f64(secs)
    }
}

fn run_output(pending: Pending) {
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
    pending: Pending,
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
                && let Some((freqs, together)) = slot.take()
            {
                synth.load(&freqs, together);
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

/// One playback segment: the frequencies sounding simultaneously and its length in samples.
/// An arpejo is a queue of one-frequency segments; a block chord is a single multi-frequency
/// segment.
struct Seg {
    freqs: Vec<f64>,
    total: usize,
}

/// Renders a queue of enveloped (poly)phonic sine segments, one sample at a time. The active
/// segment is flattened into `cur_freqs` + `phases` (sibling fields) so a sample can read a
/// frequency and advance its phase without a borrow conflict.
struct ToneSynth {
    sr: f64,
    queue: VecDeque<Seg>,
    cur_freqs: Vec<f64>,
    phases: Vec<f64>,
    left: usize,
    total: usize,
}

impl ToneSynth {
    fn new(sr: f64) -> Self {
        ToneSynth {
            sr,
            queue: VecDeque::new(),
            cur_freqs: Vec::new(),
            phases: Vec::new(),
            left: 0,
            total: 0,
        }
    }

    /// Replace the queue with new segments (clears anything still playing). `together` sounds
    /// all freqs as one block-chord segment; otherwise each freq is its own arpejo segment.
    fn load(&mut self, freqs: &[f64], together: bool) {
        self.queue.clear();
        self.cur_freqs.clear();
        self.phases.clear();
        self.left = 0;
        self.total = 0;
        if freqs.len() <= 1 || together {
            let total = (self.sr * NOTE_SECS).max(1.0) as usize;
            self.queue.push_back(Seg {
                freqs: freqs.to_vec(),
                total,
            });
        } else {
            let total = (self.sr * ARP_SECS).max(1.0) as usize;
            for &freq in freqs {
                self.queue.push_back(Seg {
                    freqs: vec![freq],
                    total,
                });
            }
        }
    }

    fn next(&mut self) -> f32 {
        if self.left == 0 {
            match self.queue.pop_front() {
                Some(seg) => {
                    self.total = seg.total;
                    self.left = seg.total;
                    self.phases = vec![0.0; seg.freqs.len()];
                    self.cur_freqs = seg.freqs;
                }
                None => {
                    self.cur_freqs.clear();
                    return 0.0;
                }
            }
        }
        if self.cur_freqs.is_empty() {
            return 0.0;
        }
        let pos = self.total - self.left;
        let ramp = ((RAMP_SECS * self.sr) as usize).max(1);
        let env = if pos < ramp {
            pos as f32 / ramp as f32
        } else if self.left < ramp {
            self.left as f32 / ramp as f32
        } else {
            1.0
        };
        let n = self.cur_freqs.len();
        let mut sample = 0.0f32;
        for i in 0..n {
            // Copy the freq out first so the immutable borrow ends before `phases[i]` is
            // mutated (the same disjoint-access reason the single-voice version compiled).
            let freq = self.cur_freqs[i];
            self.phases[i] += std::f64::consts::TAU * freq / self.sr;
            sample += self.phases[i].sin() as f32;
        }
        self.left -= 1;
        // Average the voices so a chord doesn't clip relative to a single note.
        (sample / n as f32) * env * GAIN
    }
}
