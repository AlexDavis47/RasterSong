//! Every colour the app paints itself, for the dark and the light theme. Widgets use egui's own
//! visuals (tuned in [`apply_style`]); the canvas, timeline and preview take their colours from
//! here, so nothing else holds a colour literal.

use eframe::egui::{self, Color32, CornerRadius};
use rastersong_engine::Category;
use serde::{Deserialize, Serialize};

/// The user's theme choice. Dark by default, whatever the system uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
    System,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [Self::Dark, Self::Light, Self::System];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::System => "Follow system",
        }
    }

    pub fn preference(self) -> egui::ThemePreference {
        match self {
            Self::Dark => egui::ThemePreference::Dark,
            Self::Light => egui::ThemePreference::Light,
            Self::System => egui::ThemePreference::System,
        }
    }
}

/// Colours painted by the app.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub accent: Color32,
    pub error: Color32,
    /// Playback slower than real time.
    pub warning: Color32,
    /// Hints over the preview ("Open a video to start").
    pub text_dim: Color32,

    pub canvas_bg: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub node_body: Color32,
    pub node_shadow: Color32,
    pub node_outline: Color32,
    /// Inputs that must be connected.
    pub pin_required: Color32,
    pub pin_optional: Color32,
    pub pin_outline: Color32,
    /// Node categories: Input, Structure, Convert, Effect, Output.
    pub categories: [Color32; 5],
    pub unknown_category: Color32,
    pub error_bar: Color32,
    pub error_bar_hover: Color32,
    pub error_bar_text: Color32,

    pub preview_bg: Color32,

    pub lane_bg: Color32,
    pub tick: Color32,
    pub tick_label: Color32,
    pub playhead: Color32,
    pub video_block: Color32,
    pub audio_block: Color32,
    pub block_text: Color32,
    pub cached: Color32,
}

impl Theme {
    pub const DARK: Theme = Theme {
        accent: Color32::from_rgb(0x6a, 0xa8, 0xff),
        error: Color32::from_rgb(0xff, 0x6b, 0x6b),
        warning: Color32::from_rgb(0xf0, 0xc0, 0x40),
        text_dim: Color32::from_gray(160),

        canvas_bg: Color32::from_gray(14),
        grid_minor: Color32::from_gray(48),
        grid_major: Color32::from_gray(40),
        node_body: Color32::from_gray(38),
        node_shadow: Color32::from_black_alpha(70),
        node_outline: Color32::from_gray(60),
        pin_required: Color32::from_gray(225),
        pin_optional: Color32::from_gray(120),
        pin_outline: Color32::from_gray(25),
        categories: [
            Color32::from_rgb(0x4a, 0x90, 0xd9),
            Color32::from_rgb(0xa6, 0x7c, 0xd6),
            Color32::from_rgb(0xd6, 0x5c, 0x8a),
            Color32::from_rgb(0xe0, 0x9a, 0x3c),
            Color32::from_rgb(0x5c, 0xb8, 0x5c),
        ],
        unknown_category: Color32::from_gray(120),
        error_bar: Color32::from_rgb(0x5a, 0x1e, 0x1e),
        error_bar_hover: Color32::from_rgb(0x6e, 0x24, 0x24),
        error_bar_text: Color32::from_rgb(0xff, 0xd0, 0xd0),

        preview_bg: Color32::BLACK,

        lane_bg: Color32::from_gray(30),
        tick: Color32::from_gray(110),
        tick_label: Color32::from_gray(150),
        playhead: Color32::WHITE,
        video_block: Color32::from_rgb(0x2d, 0x4f, 0x7a),
        audio_block: Color32::from_rgb(0xa8, 0x6a, 0x26),
        block_text: Color32::from_gray(20),
        cached: Color32::from_rgb(0x5c, 0xd6, 0x6a),
    };

    pub const LIGHT: Theme = Theme {
        accent: Color32::from_rgb(0x2a, 0x6c, 0xd8),
        error: Color32::from_rgb(0xc8, 0x30, 0x30),
        warning: Color32::from_rgb(0xa8, 0x70, 0x00),
        text_dim: Color32::from_gray(120),

        canvas_bg: Color32::from_gray(236),
        grid_minor: Color32::from_gray(196),
        grid_major: Color32::from_gray(214),
        node_body: Color32::from_gray(252),
        node_shadow: Color32::from_black_alpha(30),
        node_outline: Color32::from_gray(178),
        pin_required: Color32::from_gray(70),
        pin_optional: Color32::from_gray(165),
        pin_outline: Color32::from_gray(250),
        categories: [
            Color32::from_rgb(0x2f, 0x78, 0xc4),
            Color32::from_rgb(0x86, 0x58, 0xbe),
            Color32::from_rgb(0xc0, 0x40, 0x6e),
            Color32::from_rgb(0xcc, 0x7a, 0x14),
            Color32::from_rgb(0x38, 0x96, 0x38),
        ],
        unknown_category: Color32::from_gray(140),
        error_bar: Color32::from_rgb(0xf6, 0xd4, 0xd4),
        error_bar_hover: Color32::from_rgb(0xf0, 0xc2, 0xc2),
        error_bar_text: Color32::from_rgb(0x7a, 0x10, 0x10),

        preview_bg: Color32::from_gray(24),

        lane_bg: Color32::from_gray(222),
        tick: Color32::from_gray(150),
        tick_label: Color32::from_gray(95),
        playhead: Color32::from_gray(20),
        video_block: Color32::from_rgb(0x9c, 0xbc, 0xe8),
        audio_block: Color32::from_rgb(0xec, 0xb8, 0x78),
        block_text: Color32::from_gray(30),
        cached: Color32::from_rgb(0x2c, 0x9e, 0x3c),
    };

    /// The theme egui is currently showing.
    pub fn of(ctx: &egui::Context) -> &'static Theme {
        Self::for_visuals(&ctx.global_style().visuals)
    }

    pub fn for_visuals(visuals: &egui::Visuals) -> &'static Theme {
        if visuals.dark_mode {
            &Self::DARK
        } else {
            &Self::LIGHT
        }
    }

    pub fn category(&self, category: Option<Category>) -> Color32 {
        match category {
            Some(Category::Input) => self.categories[0],
            Some(Category::Structure) => self.categories[1],
            Some(Category::Convert) => self.categories[2],
            Some(Category::Effect) => self.categories[3],
            Some(Category::Output) => self.categories[4],
            None => self.unknown_category,
        }
    }
}

/// Spacing and widget visuals shared by the whole app, for both themes.
pub fn apply_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.interact_size.y = 22.0;
        style.spacing.combo_width = 120.0;
        for widgets in [
            &mut style.visuals.widgets.noninteractive,
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
        ] {
            widgets.corner_radius = CornerRadius::same(4);
        }
    });
    for (egui_theme, theme) in [
        (egui::Theme::Dark, &Theme::DARK),
        (egui::Theme::Light, &Theme::LIGHT),
    ] {
        ctx.style_mut_of(egui_theme, |style| {
            let visuals = &mut style.visuals;
            visuals.selection.bg_fill = theme.accent.gamma_multiply(0.6);
            visuals.hyperlink_color = theme.accent;
            visuals.error_fg_color = theme.error;
            visuals.warn_fg_color = theme.warning;
        });
    }
    // egui's light theme puts white text on the light selection fill; keep it readable.
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.visuals.selection.bg_fill = Theme::LIGHT.accent.gamma_multiply(0.35);
        style.visuals.selection.stroke.color = Color32::from_gray(10);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_theme_from_the_visuals() {
        assert_eq!(Theme::for_visuals(&egui::Visuals::dark()), &Theme::DARK);
        assert_eq!(Theme::for_visuals(&egui::Visuals::light()), &Theme::LIGHT);
    }

    #[test]
    fn defaults_to_dark() {
        assert_eq!(ThemeChoice::default(), ThemeChoice::Dark);
    }
}
