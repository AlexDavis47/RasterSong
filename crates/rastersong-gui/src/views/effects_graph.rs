use iced::widget::{column, container, text};
use iced::{Element, Length};

use super::style;
use crate::theme::padding;

pub struct EffectsGraph;

impl EffectsGraph {
    pub fn view<Message: 'static>() -> Element<'static, Message> {
        container(
            column![
                container(
                    text("Effects Graph")
                        .size(12)
                        .style(style::styles::text_secondary_style())
                )
                .width(Length::Fill)
                .padding([8, 12])
                .style(style::styles::panel_header_style()),
                container(
                    text("Graph viewport goes here").style(style::styles::text_primary_style())
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(padding::VIEWPORT)
                .style(style::styles::viewport_style()),
            ]
            .spacing(0),
        )
        .width(Length::FillPortion(4))
        .height(Length::Fill)
        .padding(padding::PANEL)
        .style(style::styles::panel_style())
        .into()
    }
}
