use iced::Element;
use iced::widget::{button, row};

/// Playback controls component (Play/Pause, Load, etc.)
pub struct Controls;

impl Controls {
    /// Creates a view for the playback controls
    pub fn view<'a, Message: 'a + Clone>(
        is_playing: bool,
        has_video: bool,
        on_play_pause: Message,
        on_load_video: Message,
    ) -> Element<'a, Message> {
        let play_pause_btn = if has_video {
            button(if is_playing { "Pause" } else { "Play" }).on_press(on_play_pause)
        } else {
            button("Play") // Disabled appearance
        };

        row![play_pause_btn, button("Load Video").on_press(on_load_video),]
            .spacing(10)
            .into()
    }
}
