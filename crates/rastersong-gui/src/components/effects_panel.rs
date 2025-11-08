use iced::widget::{checkbox, column, container, row, slider, text};
use iced::{Element, Length};
use rastersong::{EffectSettings, InterpolationMode};

/// Effects panel component with all audio modulation controls
pub struct EffectsPanel;

impl EffectsPanel {
    /// Creates the effects panel view
    pub fn view<'a, Message: 'a + Clone>(
        effects: &'a EffectSettings,
        has_modulator: bool,
        on_settings_change: impl Fn(EffectSettings) -> Message + 'a + Copy,
    ) -> Element<'a, Message> {
        let mut content = column![text("Audio Modulation Effects").size(18),]
            .spacing(10)
            .padding(10);

        if !has_modulator {
            content = content.push(
                container(text("Load a modulator audio file to enable effects"))
                    .padding(10)
                    .style(|_theme| container::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgb(
                            0.2, 0.2, 0.2,
                        ))),
                        border: iced::Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            );
        } else {
            // Amplitude Modulation
            content = content.push(
                column![
                    row![
                        checkbox("AM (Amplitude Modulation)", effects.am_enabled).on_toggle(
                            move |enabled| {
                                let mut s = effects.clone();
                                s.am_enabled = enabled;
                                on_settings_change(s)
                            }
                        ),
                    ],
                    if effects.am_enabled {
                        Element::from(
                            row![
                                text("Depth:").width(60),
                                slider(0.0..=1.0, effects.am_depth, move |value| {
                                    let mut s = effects.clone();
                                    s.am_depth = value;
                                    on_settings_change(s)
                                })
                                .step(0.01),
                                text(format!("{:.2}", effects.am_depth)).width(50),
                            ]
                            .spacing(10),
                        )
                    } else {
                        Element::from(row![])
                    },
                ]
                .spacing(5),
            );

            // Ring Modulation
            content = content.push(
                column![
                    row![
                        checkbox("Ring Modulation", effects.ring_mod_enabled).on_toggle(
                            move |enabled| {
                                let mut s = effects.clone();
                                s.ring_mod_enabled = enabled;
                                on_settings_change(s)
                            }
                        ),
                    ],
                    if effects.ring_mod_enabled {
                        Element::from(
                            row![
                                text("Mix:").width(60),
                                slider(0.0..=1.0, effects.ring_mod_mix, move |value| {
                                    let mut s = effects.clone();
                                    s.ring_mod_mix = value;
                                    on_settings_change(s)
                                })
                                .step(0.01),
                                text(format!("{:.2}", effects.ring_mod_mix)).width(50),
                            ]
                            .spacing(10),
                        )
                    } else {
                        Element::from(row![])
                    },
                ]
                .spacing(5),
            );

            // Frequency Modulation
            content = content.push(
                column![
                    row![
                        checkbox("FM (Frequency Modulation)", effects.fm_enabled).on_toggle(
                            move |enabled| {
                                let mut s = effects.clone();
                                s.fm_enabled = enabled;
                                on_settings_change(s)
                            }
                        ),
                    ],
                    if effects.fm_enabled {
                        Element::from(
                            row![
                                text("Depth:").width(60),
                                slider(0.0..=50.0, effects.fm_depth, move |value| {
                                    let mut s = effects.clone();
                                    s.fm_depth = value;
                                    on_settings_change(s)
                                })
                                .step(1.0),
                                text(format!("{:.0}", effects.fm_depth)).width(50),
                            ]
                            .spacing(10),
                        )
                    } else {
                        Element::from(row![])
                    },
                ]
                .spacing(5),
            );

            // Distortion
            content = content.push(
                column![
                    row![
                        checkbox("Distortion", effects.distortion_enabled).on_toggle(
                            move |enabled| {
                                let mut s = effects.clone();
                                s.distortion_enabled = enabled;
                                on_settings_change(s)
                            }
                        ),
                    ],
                    if effects.distortion_enabled {
                        Element::from(
                            row![
                                text("Amount:").width(60),
                                slider(0.0..=1.0, effects.distortion_amount, move |value| {
                                    let mut s = effects.clone();
                                    s.distortion_amount = value;
                                    on_settings_change(s)
                                })
                                .step(0.01),
                                text(format!("{:.2}", effects.distortion_amount)).width(50),
                            ]
                            .spacing(10),
                        )
                    } else {
                        Element::from(row![])
                    },
                ]
                .spacing(5),
            );

            // Bit Crush
            content = content.push(
                column![
                    row![checkbox("Bit Crush", effects.bit_crush_enabled).on_toggle(
                        move |enabled| {
                            let mut s = effects.clone();
                            s.bit_crush_enabled = enabled;
                            on_settings_change(s)
                        }
                    ),],
                    if effects.bit_crush_enabled {
                        Element::from(
                            row![
                                text("Bits:").width(60),
                                slider(1.0..=8.0, effects.bit_crush_bits as f32, move |value| {
                                    let mut s = effects.clone();
                                    s.bit_crush_bits = value as u8;
                                    on_settings_change(s)
                                })
                                .step(1.0),
                                text(format!("{}", effects.bit_crush_bits)).width(50),
                            ]
                            .spacing(10),
                        )
                    } else {
                        Element::from(row![])
                    },
                ]
                .spacing(5),
            );

            // Low Pass Filter
            content = content.push(
                column![
                    row![checkbox("Low Pass Filter", effects.lpf_enabled).on_toggle(
                        move |enabled| {
                            let mut s = effects.clone();
                            s.lpf_enabled = enabled;
                            on_settings_change(s)
                        }
                    ),],
                    if effects.lpf_enabled {
                        Element::from(
                            row![
                                text("Cutoff:").width(60),
                                slider(0.1..=20000.0, effects.lpf_cutoff, move |value| {
                                    let mut s = effects.clone();
                                    s.lpf_cutoff = value;
                                    on_settings_change(s)
                                })
                                .step(0.1),
                                text(format!("{:.0} Hz", effects.lpf_cutoff)).width(80),
                            ]
                            .spacing(10),
                        )
                    } else {
                        Element::from(row![])
                    },
                ]
                .spacing(5),
            );
            
            // Interpolation Mode (quality vs performance)
            let is_sinc = effects.interpolation == InterpolationMode::Sinc;
            content = content.push(
                column![
                    row![
                        checkbox("High Quality Interpolation (Sinc)", is_sinc).on_toggle(
                            move |enabled| {
                                let mut s = effects.clone();
                                s.interpolation = if enabled {
                                    InterpolationMode::Sinc
                                } else {
                                    InterpolationMode::Linear
                                };
                                on_settings_change(s)
                            }
                        ),
                    ],
                    row![
                        text(if is_sinc {
                            "Lanczos windowed-sinc (DAW quality, slower)"
                        } else {
                            "Linear interpolation (fast, good quality)"
                        })
                        .size(11)
                        .color(iced::Color::from_rgb(0.6, 0.6, 0.6))
                    ],
                ]
                .spacing(2),
            );
        }

        container(content)
            .width(Length::Fill)
            .padding(10)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.15, 0.15, 0.15,
                ))),
                border: iced::Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    }
}
