//! Sample-accurate metronome click output via a cpal output stream.
//!
//! Timing is driven entirely off the shared [`SharedClock`] read inside the audio
//! callback — never off the UI subscription, which would drift. Each beat crossing
//! triggers a short enveloped tone; the downbeat (beat index ≡ 0 mod bar) is
//! higher/brighter than the off-beats.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::time::{Duration, Instant};

use crate::clock::SharedClock;

/// Click length in seconds (a short percussive blip).
const CLICK_SECS: f64 = 0.035;
/// Downbeat / off-beat tone frequencies (Hz).
const DOWNBEAT_HZ: f64 = 1760.0;
const OFFBEAT_HZ: f64 = 1175.0;
/// Peak click amplitude.
const GAIN: f32 = 0.35;

/// Open the default output device and render metronome clicks off `clock` until
/// the process exits. Blocks; intended to own a dedicated thread. Clicks are
/// emitted only while `clock.running() && clock.audible()`.
pub fn run_click(clock: SharedClock) {
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        eprintln!("[beat] no output device");
        return;
    };
    let Ok(cfg) = device.default_output_config() else {
        eprintln!("[beat] no output config");
        return;
    };
    overlay::debug(
        "beat",
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
    let err_fn = |e: cpal::StreamError| eprintln!("[beat] stream error: {e}");

    use cpal::SampleFormat;
    let config = cfg.config();
    let stream = match cfg.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, clock, sr, channels, err_fn),
        SampleFormat::I16 => build::<i16>(&device, &config, clock, sr, channels, err_fn),
        SampleFormat::U16 => build::<u16>(&device, &config, clock, sr, channels, err_fn),
        other => {
            eprintln!("[beat] unsupported output sample format: {other:?}");
            return;
        }
    };
    let Ok(stream) = stream else {
        eprintln!("[beat] build_output_stream failed");
        return;
    };
    if stream.play().is_err() {
        eprintln!("[beat] stream.play failed");
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
    clock: SharedClock,
    sr: f64,
    channels: usize,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    let mut synth = ClickSynth::new(sr);
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let now = Instant::now();
            for (i, frame) in data.chunks_mut(channels).enumerate() {
                let t = now + Duration::from_secs_f64(i as f64 / sr);
                let v = T::from_sample(synth.next(&clock, t));
                for ch in frame.iter_mut() {
                    *ch = v;
                }
            }
        },
        err_fn,
        None,
    )
}

/// Per-stream click voice: detects beat crossings and renders a decaying tone.
struct ClickSynth {
    sr: f64,
    last_beat: Option<i64>,
    env_left: usize,
    env_total: usize,
    freq: f64,
    phase: f64,
}

impl ClickSynth {
    fn new(sr: f64) -> Self {
        ClickSynth {
            sr,
            last_beat: None,
            env_left: 0,
            env_total: (sr * CLICK_SECS) as usize,
            freq: 0.0,
            phase: 0.0,
        }
    }

    /// One output sample at instant `t`, triggering a new click on each beat
    /// crossing while the clock is running and audible.
    fn next(&mut self, clock: &SharedClock, t: Instant) -> f32 {
        if clock.running() && clock.audible() {
            let pos = clock.beat_position_at(t);
            if pos >= 0.0 {
                let idx = pos.floor() as i64;
                if self.last_beat != Some(idx) {
                    self.last_beat = Some(idx);
                    let bar = clock.beats_per_bar() as i64;
                    let downbeat = idx.rem_euclid(bar) == 0;
                    self.freq = if downbeat { DOWNBEAT_HZ } else { OFFBEAT_HZ };
                    self.env_left = self.env_total;
                    self.phase = 0.0;
                }
            }
        } else {
            // Stopped/muted: arm so the next crossing after resume re-clicks.
            self.last_beat = None;
        }

        if self.env_left == 0 || self.env_total == 0 {
            return 0.0;
        }
        let progress = 1.0 - self.env_left as f64 / self.env_total as f64;
        let env = (1.0 - progress) * (1.0 - progress); // quadratic decay
        self.phase += std::f64::consts::TAU * self.freq / self.sr;
        self.env_left -= 1;
        (self.phase.sin() * env) as f32 * GAIN
    }
}
