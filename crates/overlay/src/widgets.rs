//! Small shared UI widgets for the overlay apps.

use iced::widget::{container, row, text};
use iced::{Background, Border, Color, Element, Length};

/// A compact segmented level meter (green → amber → red near the top), driven by
/// a normalized `0..1` level — e.g. `pitch::level_norm(rms)` for a mic-input
/// indicator, so the user can see capture is working even before a pitch locks.
pub fn level_meter<Message: 'static>(level: f32) -> Element<'static, Message> {
    const SEGS: usize = 14;
    let lit = (level.clamp(0.0, 1.0) * SEGS as f32).round() as usize;
    let mut bars = row![].spacing(2).align_y(iced::Center);
    for i in 0..SEGS {
        let frac = i as f32 / SEGS as f32;
        let color = if i >= lit {
            Color::from_rgba(1.0, 1.0, 1.0, 0.12) // unlit
        } else if frac > 0.85 {
            Color::from_rgb(0.90, 0.25, 0.25) // near clipping
        } else if frac > 0.65 {
            Color::from_rgb(0.95, 0.75, 0.20)
        } else {
            Color::from_rgb(0.30, 0.80, 0.45)
        };
        bars = bars.push(
            container(text(""))
                .width(Length::Fixed(6.0))
                .height(Length::Fixed(8.0))
                .style(move |_theme| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        );
    }
    bars.into()
}
