mod theme;
mod views;

use iced::widget::{column, row};
use iced::{Element, Length};

use views::{EffectsGraph, Preview, Properties, Timeline, TopBar};

pub fn main() -> iced::Result {
    iced::application("RasterSong", App::update, App::view).run()
}

#[derive(Debug, Clone)]
enum Message {
    // Add your messages here
}

struct App {
    // Add your application state here
}

impl App {
    fn new() -> Self {
        Self {}
    }

    fn update(&mut self, message: Message) {
        match message {
            // Handle messages
        }
    }

    fn view(&self) -> Element<'_, Message> {
        use iced::Background;
        use iced::widget::container;

        container(
            column![
                TopBar::view(),
                row![Preview::view(), EffectsGraph::view(), Properties::view(),]
                    .spacing(0)
                    .width(Length::Fill)
                    .height(Length::Fill),
                Timeline::view(),
            ]
            .spacing(0)
            .padding(theme::padding::PANEL),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(Background::Color(theme::colors::BACKGROUND)),
            ..Default::default()
        })
        .into()
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
