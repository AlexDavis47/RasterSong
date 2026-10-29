//! Every colour the app paints itself, for the dark and the light theme. Widgets use egui's own
//! visuals (tuned in [`apply_style`]); the canvas, timeline and preview take their colours from
//! here, so nothing else holds a colour literal.

use eframe::egui::{self, Color32, CornerRadius};
use rastersong_engine::{Category, Kind, Part, Tag, TagRule};
use rastersong_lang::tr;
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
            Self::Dark => tr("theme.dark"),
            Self::Light => tr("theme.light"),
            Self::System => tr("theme.system"),
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
            Self::Solid => tr("wires.solid"),
            Self::Outline => tr("wires.outline"),
            Self::Gradient => tr("wires.gradient"),
            Self::Glow => tr("wires.glow"),
        }
    }
}

/// Wire colours: a base colour per kind of signal, and one per part of a signal (channel or
/// band). See [`Tag`].
#[derive(Debug, Clone, PartialEq)]
pub struct PortColors {
    pub video: Color32,
    pub audio: Color32,
    pub red: Color32,
    pub green: Color32,
    pub blue: Color32,
    pub left: Color32,
    pub right: Color32,
    pub low: Color32,
    pub mid: Color32,
    pub high: Color32,
}

/// The surfaces and text every panel and widget is drawn with; [`apply_style`] hands them to
/// egui's widgets. The values are the tokens in `docs/ui-v2.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct Surfaces {
    /// Behind everything: the window between the panels, wells and text fields.
    pub ground: Color32,
    /// The top bar and the status bar.
    pub bar: Color32,
    /// Panels, windows and menus.
    pub panel: Color32,
    /// The outline of a panel's card.
    pub card_border: Color32,
    /// Buttons, dropdowns and checkboxes at rest, hovered and pressed.
    pub raised: Color32,
    pub raised_hover: Color32,
    pub raised_active: Color32,
    /// Outlines of panels and controls, and of a hovered control.
    pub border: Color32,
    pub border_hover: Color32,
    /// Lines inside a panel: separators and indentation.
    pub divider: Color32,
    pub text: Color32,
    /// Labels, hints and disabled text.
    pub text_faint: Color32,
}

/// Colours painted by the app.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub surfaces: Surfaces,
    pub accent: Color32,
    pub error: Color32,
    /// Playback slower than real time.
    pub warning: Color32,
    /// Secondary text: units, hints over the preview ("Open a video to start").
    pub text_dim: Color32,

    pub canvas_bg: Color32,
    pub grid_minor: Color32,
    pub node_body: Color32,
    pub node_shadow: Color32,
    pub node_outline: Color32,
    /// The stripes on a bypassed node, and the word BYPASSED in its title row.
    pub node_hatch: Color32,
    pub bypassed: Color32,
    /// The outline of a node with a warning, and the note under it that says what's wrong.
    pub warning_outline: Color32,
    pub warning_note: Color32,
    pub warning_note_border: Color32,
    pub warning_note_text: Color32,
    /// Inputs that must be connected.
    pub pin_required: Color32,
    pub pin_optional: Color32,
    pub pin_outline: Color32,
    /// One colour per node category, in `Category::ALL` order, so a new category is a compile
    /// error here until it gets a colour.
    pub categories: [Color32; Category::ALL.len()],
    pub unknown_category: Color32,
    pub error_bar: Color32,
    pub error_bar_hover: Color32,
    pub error_bar_text: Color32,
    pub ports: PortColors,

    pub preview_bg: Color32,

    pub lane_bg: Color32,
    /// The outline around each timeline lane, so two tracks side by side read as two.
    pub lane_outline: Color32,
    pub tick: Color32,
    pub tick_label: Color32,
    pub playhead: Color32,
    pub video_block: Color32,
    pub audio_block: Color32,
    /// Graph items on the graph layers.
    pub graph_block: Color32,
    pub block_text: Color32,
    pub cached: Color32,
    /// The drag bubble that follows the pointer during a drag, and its text.
    pub bubble: Color32,
    pub bubble_text: Color32,
}

impl Theme {
    pub const DARK: Theme = Theme {
        surfaces: Surfaces {
            ground: Color32::from_rgb(0x0e, 0x0f, 0x11),
            bar: Color32::from_rgb(0x13, 0x14, 0x17),
            panel: Color32::from_rgb(0x16, 0x17, 0x1a),
            card_border: Color32::from_rgb(0x24, 0x26, 0x2b),
            raised: Color32::from_rgb(0x1c, 0x1d, 0x21),
            raised_hover: Color32::from_rgb(0x24, 0x26, 0x2b),
            raised_active: Color32::from_rgb(0x2a, 0x2c, 0x31),
            border: Color32::from_rgb(0x2a, 0x2c, 0x31),
            border_hover: Color32::from_rgb(0x3a, 0x3c, 0x42),
            divider: Color32::from_rgb(0x20, 0x22, 0x27),
            text: Color32::from_rgb(0xec, 0xed, 0xef),
            text_faint: Color32::from_rgb(0x86, 0x8a, 0x94),
        },
        accent: Color32::from_rgb(0x8f, 0xa2, 0xff),
        error: Color32::from_rgb(0xff, 0x6b, 0x6b),
        warning: Color32::from_rgb(0xf2, 0xb5, 0x44),
        text_dim: Color32::from_rgb(0xa1, 0xa4, 0xac),

        canvas_bg: Color32::from_rgb(0x12, 0x13, 0x15),
        grid_minor: Color32::from_rgb(0x26, 0x28, 0x2d),
        node_body: Color32::from_rgb(0x1e, 0x1f, 0x23),
        node_shadow: Color32::from_black_alpha(90),
        node_outline: Color32::from_rgb(0x33, 0x35, 0x3b),
        node_hatch: Color32::from_rgb(0x24, 0x25, 0x2a),
        bypassed: Color32::from_rgb(0xf2, 0xd0, 0x4b),
        warning_outline: Color32::from_rgb(0x8a, 0x6a, 0x22),
        warning_note: Color32::from_rgb(0x26, 0x23, 0x1b),
        warning_note_border: Color32::from_rgb(0x6e, 0x54, 0x20),
        warning_note_text: Color32::from_rgb(0xf4, 0xe3, 0xbd),
        pin_required: Color32::from_gray(225),
        pin_optional: Color32::from_gray(120),
        pin_outline: Color32::from_gray(25),
        categories: [
            Color32::from_rgb(0x4a, 0x90, 0xd9),
            Color32::from_rgb(0x3c, 0xb8, 0xc8),
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
            video: Color32::from_rgb(0xc9, 0xcc, 0xd3),
            audio: Color32::from_rgb(0x3f, 0xc7, 0xb4),
            red: Color32::from_rgb(0xff, 0x5d, 0x5d),
            green: Color32::from_rgb(0x4e, 0xd3, 0x7a),
            blue: Color32::from_rgb(0x5b, 0x8c, 0xff),
            left: Color32::from_rgb(0x5c, 0xc8, 0xf0),
            right: Color32::from_rgb(0xf0, 0x7c, 0xc8),
            low: Color32::from_rgb(0xff, 0x9a, 0x3c),
            mid: Color32::from_rgb(0xf2, 0xd0, 0x4b),
            high: Color32::from_rgb(0xb4, 0x8c, 0xff),
        },

        preview_bg: Color32::BLACK,

        lane_bg: Color32::from_rgb(0x1c, 0x1d, 0x21),
        lane_outline: Color32::from_rgb(0x4a, 0x4d, 0x55),
        tick: Color32::from_rgb(0x5a, 0x5d, 0x66),
        tick_label: Color32::from_rgb(0x86, 0x8a, 0x94),
        playhead: Color32::from_rgb(0x8f, 0xa2, 0xff),
        video_block: Color32::from_rgb(0x2d, 0x4f, 0x7a),
        audio_block: Color32::from_rgb(0xa8, 0x6a, 0x26),
        graph_block: Color32::from_rgb(0x9c, 0x86, 0xff),
        block_text: Color32::from_gray(20),
        cached: Color32::from_rgb(0x4e, 0xd3, 0x7a),
        bubble: Color32::from_rgb(0x3a, 0x6f, 0xc4),
        bubble_text: Color32::from_gray(250),
    };

    /// Light keeps egui's own light surfaces until it gets its UI V2 pass.
    pub const LIGHT: Theme = Theme {
        surfaces: Surfaces {
            ground: Color32::from_gray(255),
            bar: Color32::from_gray(242),
            panel: Color32::from_gray(248),
            card_border: Color32::from_gray(212),
            raised: Color32::from_gray(230),
            raised_hover: Color32::from_gray(220),
            raised_active: Color32::from_gray(165),
            border: Color32::from_gray(190),
            border_hover: Color32::from_gray(105),
            divider: Color32::from_gray(190),
            text: Color32::from_gray(60),
            text_faint: Color32::from_gray(140),
        },
        accent: Color32::from_rgb(0x2a, 0x6c, 0xd8),
        error: Color32::from_rgb(0xc8, 0x30, 0x30),
        warning: Color32::from_rgb(0xa8, 0x70, 0x00),
        text_dim: Color32::from_gray(120),

        canvas_bg: Color32::from_gray(236),
        grid_minor: Color32::from_gray(196),
        node_body: Color32::from_gray(252),
        node_shadow: Color32::from_black_alpha(30),
        node_outline: Color32::from_gray(178),
        node_hatch: Color32::from_gray(238),
        bypassed: Color32::from_rgb(0x9a, 0x78, 0x00),
        warning_outline: Color32::from_rgb(0xc8, 0x96, 0x30),
        warning_note: Color32::from_rgb(0xfd, 0xf3, 0xdc),
        warning_note_border: Color32::from_rgb(0xd8, 0xb4, 0x68),
        warning_note_text: Color32::from_rgb(0x5a, 0x42, 0x10),
        pin_required: Color32::from_gray(70),
        pin_optional: Color32::from_gray(165),
        pin_outline: Color32::from_gray(250),
        categories: [
            Color32::from_rgb(0x2f, 0x78, 0xc4),
            Color32::from_rgb(0x1f, 0x9a, 0xaa),
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
            left: Color32::from_rgb(0x18, 0x8c, 0xbc),
            right: Color32::from_rgb(0xc0, 0x40, 0x96),
            low: Color32::from_rgb(0xdc, 0x70, 0x1c),
            mid: Color32::from_rgb(0xb8, 0x98, 0x10),
            high: Color32::from_rgb(0x8a, 0x52, 0xcc),
        },

        preview_bg: Color32::from_gray(24),

        lane_bg: Color32::from_gray(222),
        lane_outline: Color32::from_gray(140),
        tick: Color32::from_gray(150),
        tick_label: Color32::from_gray(95),
        playhead: Color32::from_gray(20),
        video_block: Color32::from_rgb(0x9c, 0xbc, 0xe8),
        audio_block: Color32::from_rgb(0xec, 0xb8, 0x78),
        graph_block: Color32::from_rgb(0xc4, 0xb0, 0xe8),
        block_text: Color32::from_gray(30),
        cached: Color32::from_rgb(0x2c, 0x9e, 0x3c),
        bubble: Color32::from_rgb(0x2f, 0x6b, 0xd6),
        bubble_text: Color32::from_gray(255),
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

    /// The base colour of a kind of signal, or `None` when it isn't known.
    pub fn kind_color(&self, kind: Kind) -> Option<Color32> {
        match kind {
            Kind::Unknown => None,
            Kind::Video => Some(self.ports.video),
            Kind::Audio => Some(self.ports.audio),
        }
    }

    /// The colour of a part of a signal, or `None` for a whole signal or an unnamed channel.
    pub fn part_color(&self, part: Part) -> Option<Color32> {
        let p = &self.ports;
        match part {
            Part::Whole | Part::Channel(_) => None,
            Part::Red => Some(p.red),
            Part::Green => Some(p.green),
            Part::Blue => Some(p.blue),
            Part::Left => Some(p.left),
            Part::Right => Some(p.right),
            Part::Low => Some(p.low),
            Part::Mid => Some(p.mid),
            Part::High => Some(p.high),
        }
    }

    /// A compiled signal's wire colours: `(base, part)`.
    pub fn tag_colors(&self, tag: Tag) -> (Option<Color32>, Option<Color32>) {
        (self.kind_color(tag.kind), self.part_color(tag.part))
    }

    /// What an output's tag rule says about its wire's colours before the graph is compiled:
    /// `(base, part)`, each `None` where the rule leaves it to the main input. A part of
    /// `Some(None)` means "whole signal, no part".
    pub fn rule_colors(&self, rule: TagRule) -> (Option<Color32>, Option<Option<Color32>>) {
        (
            rule.kind.and_then(|k| self.kind_color(k)),
            rule.part.map(|p| self.part_color(p)),
        )
    }

    pub fn category(&self, category: Option<Category>) -> Color32 {
        category.map_or(self.unknown_category, |c| self.categories[c.index()])
    }
}

/// Seconds the pointer rests on a widget before its tooltip shows.
const TOOLTIP_DELAY: f32 = 0.1;

/// Height of a primary control: a dialog's main buttons and fields.
pub const CONTROL_HEIGHT: f32 = 32.0;
/// Height of every other control, including one inside a segmented control or a dense row.
pub const DENSE_HEIGHT: f32 = 26.0;
/// Corner radii of controls, of nodes and cards, of menus and of windows.
pub const CONTROL_RADIUS: u8 = 6;
pub const CARD_RADIUS: u8 = 8;
pub const MENU_RADIUS: u8 = 8;
pub const WINDOW_RADIUS: u8 = 12;
/// Text sizes that differ from egui's (body stays at its 13).
pub const SMALL_SIZE: f32 = 10.5;
const BUTTON_SIZE: f32 = 12.5;
/// Panel titles ("Preview", the open graph's name), in semibold.
pub const TITLE_SIZE: f32 = 13.0;
const MONO_SIZE: f32 = 12.5;
const HEADING_SIZE: f32 = 16.0;
/// The large timecode under the preview.
pub const TIMECODE_SIZE: f32 = 15.0;

/// The bundled faces: Inter for text (regular; medium for buttons; semibold for titles),
/// JetBrains Mono (without ligatures) for numbers. All are under the SIL Open Font License; the
/// license texts sit beside them and ship in every package.
const TEXT_FONT: &str = "Inter";
const MEDIUM_FONT: &str = "Inter Medium";
const SEMIBOLD_FONT: &str = "Inter SemiBold";
const MONO_FONT: &str = "JetBrains Mono";

/// Inter Medium: buttons, menus and tabs (egui's button text style).
pub fn medium(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(MEDIUM_FONT.into()))
}

/// Inter SemiBold: panel titles, node titles, the open graph's name.
pub fn semibold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(SEMIBOLD_FONT.into()))
}

/// egui's fonts with the bundled faces put first, so egui's own still cover the symbols and
/// emoji the bundled faces lack.
pub fn font_definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    let fallback = fonts.families[&egui::FontFamily::Proportional].clone();
    for weight in [MEDIUM_FONT, SEMIBOLD_FONT] {
        fonts
            .families
            .insert(egui::FontFamily::Name(weight.into()), fallback.clone());
    }
    for (name, data, family) in [
        (
            TEXT_FONT,
            &include_bytes!("../assets/fonts/Inter-Regular.ttf")[..],
            egui::FontFamily::Proportional,
        ),
        (
            MEDIUM_FONT,
            &include_bytes!("../assets/fonts/Inter-Medium.ttf")[..],
            egui::FontFamily::Name(MEDIUM_FONT.into()),
        ),
        (
            SEMIBOLD_FONT,
            &include_bytes!("../assets/fonts/Inter-SemiBold.ttf")[..],
            egui::FontFamily::Name(SEMIBOLD_FONT.into()),
        ),
        (
            MONO_FONT,
            &include_bytes!("../assets/fonts/JetBrainsMonoNL-Regular.ttf")[..],
            egui::FontFamily::Monospace,
        ),
    ] {
        // Unhinted, as browsers draw Inter on the boards: hinting rounds the lowercase height up
        // a whole pixel at these sizes, which makes the text look tall and narrow.
        let tweak = egui::FontTweak {
            hinting: Some(false),
            ..Default::default()
        };
        fonts.font_data.insert(
            name.to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(data).tweak(tweak)),
        );
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, name.to_owned());
    }
    fonts
}

/// Fonts, spacing and widget visuals shared by the whole app, for both themes.
pub fn apply_style(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.spacing.interact_size.y = DENSE_HEIGHT;
        for (text_style, size) in [
            (egui::TextStyle::Small, SMALL_SIZE),
            (egui::TextStyle::Monospace, MONO_SIZE),
            (egui::TextStyle::Heading, HEADING_SIZE),
        ] {
            if let Some(font) = style.text_styles.get_mut(&text_style) {
                font.size = size;
            }
        }
        // Buttons, menus and tabs in Inter Medium, as on the boards.
        style
            .text_styles
            .insert(egui::TextStyle::Button, medium(BUTTON_SIZE));
        // Tooltips appear almost at once: they carry most of the help text.
        style.interaction.tooltip_delay = TOOLTIP_DELAY;
        style.spacing.combo_width = 120.0;
        for widgets in [
            &mut style.visuals.widgets.noninteractive,
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.open,
        ] {
            widgets.corner_radius = CornerRadius::same(CONTROL_RADIUS);
        }
        // Controls are outlined at rest too, as on the boards.
        style.visuals.widgets.inactive.bg_stroke.width = 1.0;
        style.visuals.menu_corner_radius = CornerRadius::same(MENU_RADIUS);
        style.visuals.window_corner_radius = CornerRadius::same(WINDOW_RADIUS);
    });
    for (egui_theme, theme) in [
        (egui::Theme::Dark, &Theme::DARK),
        (egui::Theme::Light, &Theme::LIGHT),
    ] {
        ctx.style_mut_of(egui_theme, |style| {
            let visuals = &mut style.visuals;
            apply_surfaces(visuals, &theme.surfaces);
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

/// Gives egui's panels and widgets a theme's surfaces.
fn apply_surfaces(visuals: &mut egui::Visuals, s: &Surfaces) {
    visuals.panel_fill = s.panel;
    visuals.window_fill = s.panel;
    visuals.window_stroke.color = s.border;
    visuals.extreme_bg_color = s.ground;
    visuals.code_bg_color = s.ground;
    visuals.faint_bg_color = s.raised;
    visuals.weak_text_color = Some(s.text_faint);
    let w = &mut visuals.widgets;
    w.noninteractive.bg_fill = s.panel;
    w.noninteractive.weak_bg_fill = s.panel;
    w.noninteractive.bg_stroke.color = s.divider;
    w.noninteractive.fg_stroke.color = s.text;
    for (widget, fill, stroke) in [
        (&mut w.inactive, s.raised, s.border),
        (&mut w.hovered, s.raised_hover, s.border_hover),
        (&mut w.active, s.raised_active, s.border_hover),
        (&mut w.open, s.raised_hover, s.border),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke.color = stroke;
        widget.fg_stroke.color = s.text;
    }
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
    fn lane_outlines_stand_out_from_the_lane_fill() {
        for theme in [&Theme::DARK, &Theme::LIGHT] {
            let gap = |a: Color32, b: Color32| a.r().abs_diff(b.r());
            assert!(gap(theme.lane_outline, theme.lane_bg) >= 40);
        }
    }

    #[test]
    fn dark_widgets_use_the_ui_v2_tokens() {
        let ctx = egui::Context::default();
        apply_style(&ctx);
        ctx.set_theme(egui::Theme::Dark);
        let style = ctx.global_style();
        let v = &style.visuals;
        let s = &Theme::DARK.surfaces;
        assert_eq!(v.panel_fill, s.panel);
        assert_eq!(v.extreme_bg_color, s.ground);
        assert_eq!(v.widgets.inactive.weak_bg_fill, s.raised);
        assert_eq!(v.widgets.inactive.bg_stroke.color, s.border);
        assert_eq!(v.widgets.noninteractive.fg_stroke.color, s.text);
        assert_eq!(v.hyperlink_color, Theme::DARK.accent);
        assert_eq!(style.spacing.interact_size.y, DENSE_HEIGHT);
        assert_eq!(v.window_corner_radius, CornerRadius::same(WINDOW_RADIUS));
        assert_eq!(style.text_styles[&egui::TextStyle::Small].size, SMALL_SIZE);
        assert_eq!(
            style.text_styles[&egui::TextStyle::Button],
            medium(BUTTON_SIZE)
        );
    }

    /// WCAG contrast: at least 4.5 for text and secondary text, 3 for faint labels.
    #[test]
    fn dark_text_reads_on_every_surface() {
        fn luminance(c: Color32) -> f32 {
            let linear = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
        }
        fn contrast(a: Color32, b: Color32) -> f32 {
            let (a, b) = (luminance(a), luminance(b));
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        }
        let s = &Theme::DARK.surfaces;
        for bg in [s.ground, s.panel, s.raised, s.raised_hover, s.raised_active] {
            assert!(contrast(s.text, bg) >= 4.5);
            assert!(contrast(Theme::DARK.text_dim, bg) >= 4.5);
            assert!(contrast(s.text_faint, bg) >= 3.0);
        }
    }

    #[test]
    fn bundled_faces_come_first_and_egui_fills_in() {
        let fonts = font_definitions();
        let proportional = &fonts.families[&egui::FontFamily::Proportional];
        let monospace = &fonts.families[&egui::FontFamily::Monospace];
        assert_eq!(proportional[0], TEXT_FONT);
        assert_eq!(monospace[0], MONO_FONT);
        // egui's own faces stay behind them for symbols (♪, ▣) and emoji.
        assert!(proportional.len() > 1 && monospace.len() > 1);
        for face in [TEXT_FONT, MEDIUM_FONT, SEMIBOLD_FONT, MONO_FONT] {
            // Unhinted, as the boards draw them: hinting makes Inter look tall and narrow.
            assert_eq!(fonts.font_data[face].tweak.hinting, Some(false));
        }
        // The heavier weights fall back to the same faces as regular text.
        for weight in [MEDIUM_FONT, SEMIBOLD_FONT] {
            let family = &fonts.families[&egui::FontFamily::Name(weight.into())];
            assert_eq!(family[0], weight);
            assert_eq!(family[1..], proportional[1..]);
        }
    }

    #[test]
    fn defaults_to_dark() {
        assert_eq!(ThemeChoice::default(), ThemeChoice::Dark);
    }
}
