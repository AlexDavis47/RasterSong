use iced::widget::{column, container, text};
use iced::{Element, Length};

use super::style;
use crate::theme::padding;

pub struct Timeline;

impl Timeline {
    pub fn view<Message: 'static>() -> Element<'static, Message> {
        container(
            column![
                container(
                    text("Timeline")
                        .size(12)
                        .style(style::styles::text_secondary_style())
                )
                .width(Length::Fill)
                .padding([8, 12])
                .style(style::styles::panel_header_style()),
                container(
                    text("Timeline viewport goes here").style(style::styles::text_primary_style())
                )
                .width(Length::Fill)
                .height(Length::Fixed(200.0))
                .padding(padding::VIEWPORT)
                .style(style::styles::viewport_style()),
            ]
            .spacing(0),
        )
        .width(Length::Fill)
        .height(Length::Shrink)
        .padding(padding::PANEL)
        .style(style::styles::panel_style())
        .into()
    }
}
