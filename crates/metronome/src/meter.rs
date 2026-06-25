//! The beat-dots canvas: one dot per beat in the bar, the active beat pulsed.

use iced::mouse;
use iced::widget::canvas::{self, Frame, Path};
use iced::{Color, Point, Rectangle, Renderer, Theme};

/// Canvas program drawing a row of beat dots with the active one lit.
pub struct Beats {
    pub beats_per_bar: u32,
    /// Index of the current beat within the bar (`0` is the downbeat).
    pub active: usize,
    /// Brightness of the active beat, `0.0..=1.0` (fades between beats).
    pub pulse: f32,
    /// Accent color (shared with the rest of the overlay).
    pub color: Color,
}

impl<Message> canvas::Program<Message> for Beats {
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
        let n = self.beats_per_bar.max(1) as usize;
        let h = bounds.height;
        let w = bounds.width;
        let mid_y = h / 2.0;
        // Evenly space dots across the width.
        let step = w / (n as f32 + 1.0);
        let base_r = (h * 0.22).min(step * 0.35).max(3.0);
        let idle = Color::from_rgba(1.0, 1.0, 1.0, 0.30);

        for i in 0..n {
            let x = step * (i as f32 + 1.0);
            let is_active = i == self.active;
            let is_downbeat = i == 0;
            // Downbeats read a touch larger so the bar's "1" is findable.
            let r = base_r * if is_downbeat { 1.25 } else { 1.0 };
            let color = if is_active {
                // Lerp idle→accent by the pulse so the lit dot blooms then fades.
                let p = self.pulse.clamp(0.0, 1.0);
                Color::from_rgba(
                    idle.r + (self.color.r - idle.r) * p,
                    idle.g + (self.color.g - idle.g) * p,
                    idle.b + (self.color.b - idle.b) * p,
                    idle.a + (1.0 - idle.a) * p,
                )
            } else {
                idle
            };
            let grow = if is_active {
                1.0 + 0.5 * self.pulse.clamp(0.0, 1.0)
            } else {
                1.0
            };
            frame.fill(&Path::circle(Point::new(x, mid_y), r * grow), color);
        }

        vec![frame.into_geometry()]
    }
}
