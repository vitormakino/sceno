//! Canvas pitch-lane: note bars positioned by pitch (y) and time (x), scrolling
//! past a fixed playhead, with an optional cursor for the user's sung pitch.

use iced::mouse;
use iced::widget::canvas::{self, Frame, Path, Stroke, Text};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Size, Theme};

/// Seconds of already-passed / upcoming time shown in the lane window.
pub const PAST_SECS: f64 = 1.0;
pub const AHEAD_SECS: f64 = 4.0;

const PAD: f32 = 10.0;

/// One target note reduced to the geometry the lane needs (cheap to rebuild
/// each frame, so the lane owns these rather than borrowing the song).
#[derive(Clone, Copy)]
pub struct Bar {
    pub start: f64,
    pub end: f64,
    pub midi: f64,
    pub golden: bool,
    /// Note name + octave for the on-bar label (e.g. `"G", 4`).
    pub name: &'static str,
    pub octave: i32,
}

/// Canvas program drawing the scrolling pitch lane for the current moment.
pub struct Lane {
    pub bars: Vec<Bar>,
    /// Current playback time (seconds), at the playhead.
    pub t: f64,
    /// Visible MIDI range [lo, hi].
    pub lo: f64,
    pub hi: f64,
    /// The user's sung MIDI pitch, if any (Phase 2); colored by `cursor_color`.
    pub sung: Option<f64>,
    /// Color for the sung-pitch cursor (green when in tune).
    pub cursor_color: Color,
}

impl<Message> canvas::Program<Message> for Lane {
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
        let span = (PAST_SECS + AHEAD_SECS) as f32;
        let range = (self.hi - self.lo).max(1.0) as f32;
        let plot_h = (h - 2.0 * PAD).max(1.0);
        let band = (plot_h / range).clamp(6.0, 18.0);

        let x_of = |time: f64| ((time - (self.t - PAST_SECS)) as f32 / span) * w;
        let y_of = |midi: f64| PAD + (1.0 - (midi - self.lo) as f32 / range) * plot_h;

        // Playhead: a fixed vertical line one-third in from the left.
        let head_x = x_of(self.t);
        frame.stroke(
            &Path::line(Point::new(head_x, 0.0), Point::new(head_x, h)),
            Stroke::default()
                .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.5))
                .with_width(2.0),
        );

        const BLUE: Color = Color::from_rgb(0.55, 0.70, 1.0);
        const GOLD: Color = Color::from_rgb(1.0, 0.84, 0.30);
        const ACTIVE: Color = Color::from_rgb(0.95, 0.98, 1.0);

        for bar in &self.bars {
            let x0 = x_of(bar.start).max(0.0);
            let x1 = x_of(bar.end).min(w);
            if x1 <= 0.0 || x0 >= w || x1 <= x0 {
                continue;
            }
            let active = bar.start <= self.t && self.t < bar.end;
            let color = if active {
                ACTIVE
            } else if bar.golden {
                GOLD
            } else {
                BLUE
            };
            let y = y_of(bar.midi);
            frame.fill_rectangle(
                Point::new(x0, y - band / 2.0),
                Size::new((x1 - x0).max(2.0), band),
                color,
            );
            // Label the bar with its note name so the player knows what to sing.
            frame.fill_text(Text {
                content: format!("{}{}", bar.name, bar.octave),
                position: Point::new(x0 + 3.0, (y - band / 2.0 - 13.0).max(0.0)),
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.9),
                size: Pixels(11.0),
                ..Text::default()
            });
        }

        // Sung-pitch cursor: a full-width line at your pitch (line it up with a
        // bar = in tune) plus a solid marker at the playhead.
        if let Some(midi) = self.sung {
            let y = y_of(midi.clamp(self.lo, self.hi));
            frame.stroke(
                &Path::line(Point::new(0.0, y), Point::new(w, y)),
                Stroke::default()
                    .with_color(Color {
                        a: 0.45,
                        ..self.cursor_color
                    })
                    .with_width(2.0),
            );
            frame.fill(&Path::circle(Point::new(head_x, y), 6.0), self.cursor_color);
        }

        vec![frame.into_geometry()]
    }
}
