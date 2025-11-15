use iced::widget::container;
use iced::{Background, Border, Color};

/// Color palette for RasterSong
pub mod colors {
    use super::Color;

    // Background colors
    pub const BACKGROUND: Color = Color::from_rgb(0.102, 0.102, 0.102);
    pub const PANEL: Color = Color::from_rgb(0.145, 0.145, 0.145);
    pub const PANEL_HEADER: Color = Color::from_rgb(0.145, 0.145, 0.145);
    pub const VIEWPORT: Color = Color::from_rgb(0.145, 0.145, 0.145);

    // Border colors
    pub const BORDER: Color = Color::from_rgb(0.165, 0.165, 0.165);

    // Text colors
    pub const TEXT_PRIMARY: Color = Color::from_rgb(0.6, 0.6, 0.6);
    pub const TEXT_SECONDARY: Color = Color::from_rgb(0.6, 0.6, 0.6);
}

/// Padding constants
pub mod padding {
    pub const PANEL: f32 = 1.0;
    pub const VIEWPORT: f32 = 1.0;
    pub const MENU_ITEM: [f32; 2] = [4.0, 12.0];
    pub const TOP_BAR: [f32; 2] = [4.0, 8.0];
}

/// Style functions that return closures compatible with Iced's style API
pub mod styles {
    use super::*;

    pub fn panel_style() -> impl Fn(&iced::Theme) -> container::Style {
        |_theme| container::Style {
            background: Some(Background::Color(colors::PANEL)),
            border: Border {
                width: 1.0,
                color: colors::BORDER,
                radius: 0.0.into(),
            },
            ..Default::default()
        }
    }

    pub fn viewport_style() -> impl Fn(&iced::Theme) -> container::Style {
        |_theme| container::Style {
            background: Some(Background::Color(colors::VIEWPORT)),
            border: Border {
                width: 1.0,
                color: colors::BORDER,
                radius: 0.0.into(),
            },
            ..Default::default()
        }
    }

    pub fn panel_header_style() -> impl Fn(&iced::Theme) -> container::Style {
        |_theme| container::Style {
            background: Some(Background::Color(colors::PANEL_HEADER)),
            border: Border {
                width: 1.0,
                color: colors::BORDER,
                radius: 0.0.into(),
            },
            ..Default::default()
        }
    }

    pub fn menu_item_style() -> impl Fn(&iced::Theme) -> container::Style {
        |_theme| container::Style {
            background: None,
            border: Border::default(),
            ..Default::default()
        }
    }

    pub fn text_primary_style() -> impl Fn(&iced::Theme) -> iced::widget::text::Style {
        |_theme| iced::widget::text::Style {
            color: Some(colors::TEXT_PRIMARY),
        }
    }

    pub fn text_secondary_style() -> impl Fn(&iced::Theme) -> iced::widget::text::Style {
        |_theme| iced::widget::text::Style {
            color: Some(colors::TEXT_SECONDARY),
        }
    }
}
