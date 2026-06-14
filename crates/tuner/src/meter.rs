//! Tuning-meter styles and color feedback. Canvas drawing is added later.

use iced::mouse;
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

/// Width (px) of one strobe band; shared by the drawing and the animation step.
pub const STROBE_BAND: f32 = 24.0;

/// The visual style of the tuning meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MeterStyle {
    #[default]
    Needle,
    CenterBar,
    Strobe,
}

impl MeterStyle {
    pub fn index(self) -> usize {
        match self {
            MeterStyle::Needle => 0,
            MeterStyle::CenterBar => 1,
            MeterStyle::Strobe => 2,
        }
    }

    pub fn from_idx(i: usize) -> Self {
        match i {
            1 => MeterStyle::CenterBar,
            2 => MeterStyle::Strobe,
            _ => MeterStyle::Needle,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MeterStyle::Needle => "Agulha",
            MeterStyle::CenterBar => "Barra",
            MeterStyle::Strobe => "Strobe",
        }
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Green within ±5¢, blending to amber by ±25¢ and red by ±50¢ (symmetric).
pub fn cents_color(cents: f64) -> Color {
    const GREEN: [f32; 3] = [0.30, 0.90, 0.30];
    const AMBER: [f32; 3] = [0.95, 0.75, 0.20];
    const RED: [f32; 3] = [0.90, 0.25, 0.25];
    let c = cents.abs();
    let rgb = if c <= 5.0 {
        GREEN
    } else if c <= 25.0 {
        lerp(GREEN, AMBER, ((c - 5.0) / 20.0) as f32)
    } else {
        lerp(AMBER, RED, (((c - 25.0) / 25.0).min(1.0)) as f32)
    };
    Color::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Canvas program drawing the meter for the current reading.
pub struct Meter {
    pub cents: f64,
    pub style: MeterStyle,
    pub phase: f32,
    pub color: Color,
}

impl<Message> canvas::Program<Message> for Meter {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        let mid_x = w / 2.0;
        let mid_y = h / 2.0;
        let half = (w / 2.0 - 6.0).max(1.0);
        let track = Color::from_rgba(1.0, 1.0, 1.0, 0.35);

        // Baseline + center reference (drawn for Needle/CenterBar).
        if self.style != MeterStyle::Strobe {
            frame.stroke(
                &Path::line(Point::new(6.0, mid_y), Point::new(w - 6.0, mid_y)),
                Stroke::default().with_color(track).with_width(2.0),
            );
            frame.stroke(
                &Path::line(Point::new(mid_x, 2.0), Point::new(mid_x, h - 2.0)),
                Stroke::default().with_color(track).with_width(2.0),
            );
        }

        let pos = (self.cents.clamp(-50.0, 50.0) / 50.0) as f32;
        match self.style {
            MeterStyle::Needle => {
                let x = mid_x + pos * half;
                frame.stroke(
                    &Path::line(Point::new(x, 2.0), Point::new(x, h - 2.0)),
                    Stroke::default().with_color(self.color).with_width(4.0),
                );
            }
            MeterStyle::CenterBar => {
                let x = mid_x + pos * half;
                let (left, right) = if x >= mid_x { (mid_x, x) } else { (x, mid_x) };
                frame.fill_rectangle(
                    Point::new(left, mid_y - 4.0),
                    Size::new((right - left).max(1.0), 8.0),
                    self.color,
                );
            }
            MeterStyle::Strobe => {
                // Repeating vertical bands scrolled by `phase`; they appear to
                // freeze near 0¢ because `phase` barely advances there (see update).
                let offset = self.phase.rem_euclid(STROBE_BAND);
                let mut x = -STROBE_BAND + offset;
                while x < w {
                    frame.fill_rectangle(
                        Point::new(x, 2.0),
                        Size::new(STROBE_BAND / 2.0, h - 4.0),
                        self.color,
                    );
                    x += STROBE_BAND;
                }
            }
        }

        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_index_roundtrips() {
        for s in [
            MeterStyle::Needle,
            MeterStyle::CenterBar,
            MeterStyle::Strobe,
        ] {
            assert_eq!(MeterStyle::from_idx(s.index()), s);
        }
    }

    #[test]
    fn style_unknown_idx_defaults_to_needle() {
        assert_eq!(MeterStyle::from_idx(99), MeterStyle::Needle);
    }

    #[test]
    fn in_tune_is_green() {
        let g = cents_color(0.0);
        assert!((g.g - 0.90).abs() < 1e-6 && g.r < 0.4, "{g:?}");
        assert_eq!(cents_color(4.9), cents_color(0.0));
    }

    #[test]
    fn far_out_is_red_and_clamped() {
        let r = cents_color(50.0);
        assert!(r.r > 0.85 && r.g < 0.3, "{r:?}");
        assert_eq!(cents_color(80.0), cents_color(50.0));
    }

    #[test]
    fn color_is_symmetric() {
        let a = cents_color(-20.0);
        let b = cents_color(20.0);
        assert!((a.r - b.r).abs() < 1e-6 && (a.g - b.g).abs() < 1e-6);
    }
}
