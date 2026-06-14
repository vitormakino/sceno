//! Tuning-feedback color: how far a pitch is from its target, as a color.
//!
//! Returns an `[r, g, b]` triple (0..1) so it stays UI-framework-agnostic; the
//! apps wrap it in their renderer's color type.

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Green within ±5¢, blending to amber by ±25¢ and red by ±50¢ (symmetric).
pub fn cents_color(cents: f64) -> [f32; 3] {
    const GREEN: [f32; 3] = [0.30, 0.90, 0.30];
    const AMBER: [f32; 3] = [0.95, 0.75, 0.20];
    const RED: [f32; 3] = [0.90, 0.25, 0.25];
    let c = cents.abs();
    if c <= 5.0 {
        GREEN
    } else if c <= 25.0 {
        lerp(GREEN, AMBER, ((c - 5.0) / 20.0) as f32)
    } else {
        lerp(AMBER, RED, (((c - 25.0) / 25.0).min(1.0)) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_tune_is_green() {
        let g = cents_color(0.0);
        assert!((g[1] - 0.90).abs() < 1e-6 && g[0] < 0.4, "{g:?}");
        assert_eq!(cents_color(4.9), cents_color(0.0));
    }

    #[test]
    fn far_out_is_red_and_clamped() {
        let r = cents_color(50.0);
        assert!(r[0] > 0.85 && r[1] < 0.3, "{r:?}");
        assert_eq!(cents_color(80.0), cents_color(50.0));
    }

    #[test]
    fn color_is_symmetric() {
        let a = cents_color(-20.0);
        let b = cents_color(20.0);
        assert!((a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6);
    }
}
