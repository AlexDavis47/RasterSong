use iced::Element;
use iced::widget::{column, slider, text};

/// Timeline scrubber and time display component
pub struct Timeline;

impl Timeline {
    /// Creates a view for the timeline (slider + time display)
    pub fn view<'a, Message: 'a + Clone>(
        position: f32,
        duration: f32,
        on_seek: impl Fn(f32) -> Message + 'a,
    ) -> Element<'a, Message> {
        if duration > 0.0 {
            column![
                slider(0.0..=duration, position, on_seek).step(0.01),
                text(format!("{:.1}s / {:.1}s", position, duration)),
            ]
            .spacing(5)
            .into()
        } else {
            // Don't show timeline if no valid duration
            column![].into()
        }
    }
}
