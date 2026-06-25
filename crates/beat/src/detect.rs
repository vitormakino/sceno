//! Best-effort tempo estimation from the system-audio monitor.
//!
//! This captures a loopback ("monitor") input device, builds an energy-flux onset
//! envelope, and autocorrelates it to find the dominant beat period. It yields a
//! *tempo* estimate, not a tight phase lock — real-time beat tracking of arbitrary
//! music is inherently approximate. Pure Rust, no C deps (no aubio), per the repo
//! convention. The detector calls a `sink(BpmEstimate) -> bool` that returns
//! `false` to stop (its receiver was dropped).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Short-time energy frame and hop sizes (samples) for the onset envelope.
pub const FRAME: usize = 1024;
pub const HOP: usize = 512;
/// Tempo search range (BPM).
const BPM_RANGE: (f64, f64) = (60.0, 180.0);
/// Analysis window held for autocorrelation (seconds of audio).
const HISTORY_SECS: f64 = 8.0;
/// How often the analysis loop re-estimates.
const ANALYSIS_INTERVAL: Duration = Duration::from_millis(250);

/// A tempo estimate from the monitored audio.
#[derive(Debug, Clone, Copy)]
pub struct BpmEstimate {
    pub bpm: f64,
    /// Rough 0..1 confidence from the autocorrelation peak height.
    pub confidence: f64,
}

/// Energy-flux onset envelope: per-frame positive change in short-time energy.
pub fn onset_envelope(samples: &[f32], frame: usize, hop: usize) -> Vec<f32> {
    if frame == 0 || hop == 0 || samples.len() < frame {
        return Vec::new();
    }
    let mut env = Vec::new();
    let mut prev = 0.0f32;
    let mut i = 0;
    while i + frame <= samples.len() {
        let energy: f32 = samples[i..i + frame].iter().map(|s| s * s).sum::<f32>() / frame as f32;
        env.push((energy - prev).max(0.0));
        prev = energy;
        i += hop;
    }
    env
}

/// Autocorrelate an onset envelope sampled at `env_rate` frames/sec and return the
/// `(bpm, confidence)` at the strongest lag inside `bpm_range`.
pub fn estimate_bpm(env: &[f32], env_rate: f64, bpm_range: (f64, f64)) -> Option<(f64, f64)> {
    if env.len() < 4 || env_rate <= 0.0 {
        return None;
    }
    let mean = env.iter().copied().map(f64::from).sum::<f64>() / env.len() as f64;
    let v: Vec<f64> = env.iter().map(|&x| x as f64 - mean).collect();

    // Faster tempo → shorter lag.
    let lag_min = (env_rate * 60.0 / bpm_range.1).round() as usize;
    let lag_max = ((env_rate * 60.0 / bpm_range.0).round() as usize).min(v.len() - 1);
    if lag_min < 1 || lag_min >= lag_max {
        return None;
    }
    let total_energy: f64 = v.iter().map(|x| x * x).sum();
    if total_energy <= 0.0 {
        return None;
    }

    let mut best_lag = 0usize;
    let mut best = f64::MIN;
    for lag in lag_min..=lag_max {
        let mut acc = 0.0;
        for i in lag..v.len() {
            acc += v[i] * v[i - lag];
        }
        let norm = acc / (v.len() - lag) as f64; // per-overlap, so long lags aren't penalised
        if norm > best {
            best = norm;
            best_lag = lag;
        }
    }
    if best_lag == 0 {
        return None;
    }
    let bpm = env_rate * 60.0 / best_lag as f64;
    let confidence = (best / (total_energy / v.len() as f64)).clamp(0.0, 1.0);
    Some((bpm, confidence))
}

/// Capture the system-audio monitor and stream tempo estimates to `sink`. Blocks;
/// intended to own a dedicated thread. Returns when `sink` returns `false` or no
/// monitor device can be opened.
pub fn run_detect(mut sink: impl FnMut(BpmEstimate) -> bool) {
    let host = cpal::default_host();
    let Some(device) = monitor_device(&host) else {
        eprintln!("[beat] no monitor input device for detection");
        return;
    };
    let Ok(cfg) = device.default_input_config() else {
        eprintln!("[beat] no monitor input config");
        return;
    };
    let sr = cfg.sample_rate().0 as f64;
    let channels = cfg.channels() as usize;
    let cap = (sr * HISTORY_SECS) as usize;
    overlay::debug(
        "beat",
        format_args!(
            "detect input: {} @ {sr} Hz, {channels} ch, {:?}",
            device.name().unwrap_or_else(|_| "?".into()),
            cfg.sample_format()
        ),
    );

    let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(cap)));
    let err_fn = |e: cpal::StreamError| eprintln!("[beat] detect stream error: {e}");

    use cpal::SampleFormat;
    let config = cfg.config();
    let stream = match cfg.sample_format() {
        SampleFormat::F32 => {
            build_input::<f32>(&device, &config, buf.clone(), channels, cap, err_fn)
        }
        SampleFormat::I16 => {
            build_input::<i16>(&device, &config, buf.clone(), channels, cap, err_fn)
        }
        SampleFormat::U16 => {
            build_input::<u16>(&device, &config, buf.clone(), channels, cap, err_fn)
        }
        other => {
            eprintln!("[beat] unsupported monitor sample format: {other:?}");
            return;
        }
    };
    let Ok(stream) = stream else {
        eprintln!("[beat] build monitor stream failed");
        return;
    };
    if stream.play().is_err() {
        eprintln!("[beat] monitor stream.play failed");
        return;
    }

    let env_rate = sr / HOP as f64;
    loop {
        std::thread::sleep(ANALYSIS_INTERVAL);
        let samples = { buf.lock().unwrap().clone() };
        let env = onset_envelope(&samples, FRAME, HOP);
        if let Some((bpm, confidence)) = estimate_bpm(&env, env_rate, BPM_RANGE) {
            overlay::debug(
                "beat",
                format_args!("detected {bpm:.1} bpm (conf {confidence:.2})"),
            );
            if !sink(BpmEstimate { bpm, confidence }) {
                break;
            }
        }
    }
    drop(stream);
}

/// A cpal input device that loops back system output — its name contains
/// "monitor" (PulseAudio/PipeWire). Falls back to the default input device.
fn monitor_device(host: &cpal::Host) -> Option<cpal::Device> {
    if let Ok(devices) = host.input_devices() {
        for d in devices {
            if let Ok(name) = d.name()
                && name.to_lowercase().contains("monitor")
            {
                overlay::debug("beat", format_args!("monitor device: {name}"));
                return Some(d);
            }
        }
    }
    host.default_input_device()
}

/// Build a mono-downmixing input stream that keeps the last `cap` samples.
fn build_input<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buf: Arc<Mutex<Vec<f32>>>,
    channels: usize,
    cap: usize,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + cpal::Sample,
    f32: FromSample<T>,
{
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut b = buf.lock().unwrap();
            for frame in data.chunks(channels) {
                b.push(f32::from_sample(frame[0]));
            }
            let len = b.len();
            if len > cap {
                b.drain(0..len - cap);
            }
        },
        err_fn,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_bpm_recovers_a_click_train() {
        // Envelope sampled at 100 frames/sec with an impulse every 50 frames →
        // period 0.5 s → 120 BPM.
        let env_rate = 100.0;
        let period = 50;
        let mut env = vec![0.0f32; 800];
        let mut i = 0;
        while i < env.len() {
            env[i] = 1.0;
            i += period;
        }
        let (bpm, conf) = estimate_bpm(&env, env_rate, (60.0, 180.0)).unwrap();
        assert!((bpm - 120.0).abs() < 5.0, "got {bpm}");
        assert!(conf > 0.0);
    }

    #[test]
    fn onset_envelope_marks_energy_jumps() {
        // Silence then a loud block → a positive flux at the onset frame.
        let mut samples = vec![0.0f32; 4096];
        for s in samples.iter_mut().skip(2048) {
            *s = 0.5;
        }
        let env = onset_envelope(&samples, 1024, 512);
        assert!(!env.is_empty());
        assert!(env.iter().cloned().fold(0.0f32, f32::max) > 0.0);
    }

    #[test]
    fn estimate_bpm_rejects_flat_input() {
        assert!(estimate_bpm(&[0.0; 200], 100.0, (60.0, 180.0)).is_none());
    }
}
