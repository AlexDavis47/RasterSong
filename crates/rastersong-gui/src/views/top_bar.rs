use iced::widget::{container, row, text};
use iced::{Element, Length};

use super::style;
use crate::theme::padding;

pub struct TopBar;

impl TopBar {
    pub fn view<Message: 'static>() -> Element<'static, Message> {
        container(
            row![
                container(text("File").style(style::styles::text_primary_style()))
                    .padding(padding::MENU_ITEM)
                    .style(style::styles::menu_item_style()),
                container(text("Settings").style(style::styles::text_primary_style()))
                    .padding(padding::MENU_ITEM)
                    .style(style::styles::menu_item_style()),
                container(text("Help").style(style::styles::text_primary_style()))
                    .padding(padding::MENU_ITEM)
                    .style(style::styles::menu_item_style()),
            ]
            .spacing(0),
        )
        .width(Length::Fill)
        .height(Length::Shrink)
        .padding(padding::TOP_BAR)
        .style(style::styles::panel_style())
        .into()
    }
}
