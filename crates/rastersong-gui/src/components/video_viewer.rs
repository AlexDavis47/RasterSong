use iced::widget::{container, image, text};
use iced::{Element, Length};

/// Displays the current video frame or a placeholder
pub struct VideoViewer;

impl VideoViewer {
    /// Creates a view for the video display area
    pub fn view<'a, Message: 'a>(current_frame: &Option<image::Handle>) -> Element<'a, Message> {
        if let Some(handle) = current_frame {
            // Display the video frame
            image(handle.clone())
                .width(Length::Fill)
                .height(Length::FillPortion(8))
                .into()
        } else {
            // Display placeholder when no video is loaded
            container(text("No video loaded"))
                .width(Length::Fill)
                .height(Length::FillPortion(8))
                .center(Length::Fill)
                .into()
        }
    }
}
