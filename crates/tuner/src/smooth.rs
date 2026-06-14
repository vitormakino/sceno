//! EMA smoothing + a short "hold" for the detected pitch, kept pure for testing.

/// EMA factor (0..1; higher = snappier, lower = smoother).
const ALPHA: f64 = 0.25;
/// Consecutive `None` analysis frames tolerated before the pitch is dropped.
/// At ~50 ms/frame, 6 frames ≈ 300 ms.
const HOLD_FRAMES: u32 = 6;

/// Smooths a stream of raw per-frame frequency estimates.
#[derive(Default)]
pub struct Smoother {
    ema: Option<f64>,
    misses: u32,
}

impl Smoother {
    /// Feed the latest raw frequency (`None` = no clear pitch this frame).
    /// Returns the smoothed frequency to display, or `None` once the hold
    /// window is exceeded. Resets after a real silence so the next onset is
    /// not dragged toward the old pitch.
    pub fn update(&mut self, raw: Option<f64>) -> Option<f64> {
        match raw {
            Some(f) => {
                self.misses = 0;
                let next = match self.ema {
                    Some(prev) => ALPHA * f + (1.0 - ALPHA) * prev,
                    None => f,
                };
                self.ema = Some(next);
                Some(next)
            }
            None => {
                self.misses += 1;
                if self.misses > HOLD_FRAMES {
                    self.ema = None;
                    None
                } else {
                    self.ema
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_reading_passes_through() {
        let mut s = Smoother::default();
        assert_eq!(s.update(Some(440.0)), Some(440.0));
    }

    #[test]
    fn ema_blends_subsequent_readings() {
        let mut s = Smoother::default();
        s.update(Some(440.0));
        let v = s.update(Some(450.0)).unwrap();
        assert!((v - 442.5).abs() < 1e-9, "got {v}");
    }

    #[test]
    fn holds_last_value_then_drops() {
        let mut s = Smoother::default();
        s.update(Some(440.0));
        for _ in 0..HOLD_FRAMES {
            assert_eq!(s.update(None), Some(440.0));
        }
        assert_eq!(s.update(None), None);
    }

    #[test]
    fn restarts_cold_after_drop() {
        let mut s = Smoother::default();
        s.update(Some(440.0));
        for _ in 0..=HOLD_FRAMES {
            s.update(None);
        }
        assert_eq!(s.update(Some(300.0)), Some(300.0));
    }
}
