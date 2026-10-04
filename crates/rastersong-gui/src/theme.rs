//! Every colour the app paints itself, for the dark and the light theme. Widgets use egui's own
//! visuals (tuned in [`apply_style`]); the canvas, timeline and preview take their colours from
//! here, so nothing else holds a colour literal.

use eframe::egui::{self, Color32, CornerRadius};
use rastersong_engine::{Category, PortHint};
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

/// How wires are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WireStyle {
    /// One flat colour.
    #[default]
    Solid,
    /// The signal's kind in the middle, outlined in the part it carries (e.g. red).
    Outline,
    /// The part it carries down the centre, fading to the signal's kind at the edges.
    Gradient,
    /// The signal's kind as a crisp line, glowing in the part it carries.
    Glow,
}

impl WireStyle {
    pub const ALL: [WireStyle; 4] = [Self::Solid, Self::Outline, Self::Gradient, Self::Glow];

    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Outline => "Outlined",
            Self::Gradient => "Gradient",
            Self::Glow => "Glow",
        }
    }
}

/// Wire colours: a base colour per kind of signal, and one per part of a signal (channel or
/// band). See [`PortHint`].
#[derive(Debug, Clone, PartialEq)]
pub struct PortColors {
    pub video: Color32,
    pub audio: Color32,
    pub red: Color32,
    pub green: Color32,
    pub blue: Color32,
    pub low: Color32,
    pub mid: Color32,
    pub high: Color32,
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
    pub ports: PortColors,

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
        ports: PortColors {
            video: Color32::from_gray(205),
            audio: Color32::from_rgb(0x3c, 0xc4, 0xbc),
            red: Color32::from_rgb(0xe8, 0x55, 0x4e),
            green: Color32::from_rgb(0x4f, 0xc4, 0x5c),
            blue: Color32::from_rgb(0x4d, 0x8e, 0xf0),
            low: Color32::from_rgb(0xf0, 0x8a, 0x3a),
            mid: Color32::from_rgb(0xe6, 0xcc, 0x4a),
            high: Color32::from_rgb(0xb0, 0x7c, 0xec),
        },

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
        ports: PortColors {
            video: Color32::from_gray(80),
            audio: Color32::from_rgb(0x14, 0x9a, 0x92),
            red: Color32::from_rgb(0xd2, 0x3c, 0x34),
            green: Color32::from_rgb(0x2e, 0x9e, 0x3c),
            blue: Color32::from_rgb(0x2c, 0x6c, 0xd8),
            low: Color32::from_rgb(0xdc, 0x70, 0x1c),
            mid: Color32::from_rgb(0xb8, 0x98, 0x10),
            high: Color32::from_rgb(0x8a, 0x52, 0xcc),
        },

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

    /// What a hint says about a wire's colours: `(base, part)`, each `None` where the hint leaves
    /// it to the main input. A part of `Some(None)` means "whole signal, no part".
    pub fn hint_colors(&self, hint: PortHint) -> (Option<Color32>, Option<Option<Color32>>) {
        let p = &self.ports;
        match hint {
            PortHint::Inherit => (None, None),
            PortHint::Rgb => (Some(p.video), Some(None)),
            PortHint::Audio => (Some(p.audio), Some(None)),
            PortHint::Red => (Some(p.video), Some(Some(p.red))),
            PortHint::Green => (Some(p.video), Some(Some(p.green))),
            PortHint::Blue => (Some(p.video), Some(Some(p.blue))),
            PortHint::Low => (None, Some(Some(p.low))),
            PortHint::Mid => (None, Some(Some(p.mid))),
            PortHint::High => (None, Some(Some(p.high))),
            PortHint::AsAudio => (Some(p.audio), None),
            PortHint::AsVideo => (Some(p.video), None),
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

/// Seconds the pointer rests on a widget before its tooltip shows.
const TOOLTIP_DELAY: f32 = 0.1;

/// Spacing and widget visuals shared by the whole app, for both themes.
pub fn apply_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.interact_size.y = 22.0;
        // Tooltips appear almost at once: they carry most of the help text.
        style.interaction.tooltip_delay = TOOLTIP_DELAY;
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
