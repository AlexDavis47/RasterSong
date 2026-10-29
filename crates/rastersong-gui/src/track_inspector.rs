//! The inspector for a selected video or audio track, in the inspector's one layout: the kind
//! with the track's kind, its name and what it plays, then how it goes into the mix.

use eframe::egui::{self, RichText, Ui};
use rastersong_engine::{Project, TrackKind};
use rastersong_lang::{tr, tr_args};

use crate::editor::param_field::{NumberRange, param_field};
use crate::name_edit::name_edit;
use crate::theme::{self, Theme};
use crate::timeline::TrackAction;
use crate::widgets::{
    CHOICE_WIDTH, about, inspector_body, inspector_header, kind_row, name_style, section,
    setting_row, setting_toggle,
};

/// Shows the inspector of the track in row `row` (of [`Project::tracks`]) and returns what the
/// user changed, as the timeline's track header would.
pub fn track_inspector(
    ui: &mut Ui,
    project: &Project,
    buses: &[String],
    row: usize,
) -> Vec<TrackAction> {
    let mut actions = Vec::new();
    let Some(track) = project.tracks().nth(row) else {
        return actions;
    };
    let theme = Theme::of(ui.ctx());
    let (kind, color) = match track.kind {
        TrackKind::Video => (tr("resources.kind.video"), theme.video_block),
        TrackKind::Audio => (tr("resources.kind.audio"), theme.audio_block),
    };
    inspector_header(ui, |ui| {
        kind_row(ui, tr("inspector.kind.track"), Some((kind, color)), |_| {});
        let edit = name_edit(
            ui,
            egui::Id::new("track-inspector-name"),
            &track.name,
            name_style,
        );
        edit.response.on_hover_text(tr("timeline.track.name.help"));
        if let Some(name) = edit.committed {
            actions.push(TrackAction::Rename(row, name));
        }
        let resource = track
            .resource
            .and_then(|id| project.resource(id))
            .map(|r| r.name.as_str());
        let line = match resource {
            Some(resource) if track.items.len() == 1 => {
                tr_args("inspector.track.about_one", &[("resource", resource)])
            }
            Some(resource) => tr_args(
                "inspector.track.about",
                &[
                    ("resource", resource),
                    ("items", &track.items.len().to_string()),
                ],
            ),
            None => tr("inspector.track.empty").to_owned(),
        };
        about(ui, &line);
    });
    inspector_body(ui, |ui| {
        section(ui, tr("inspector.track.mix"), |ui| {
            if track.kind == TrackKind::Audio {
                let mut percent = f64::from(track.volume) * 100.0;
                let range = NumberRange {
                    default: 100.0,
                    soft: (0.0, 100.0),
                    limits: (0.0, 100.0),
                    whole: false,
                };
                let name = tr("inspector.track.volume");
                let field = param_field(
                    ui,
                    "track-volume",
                    name,
                    &mut percent,
                    range,
                    None,
                    &mut None,
                    |ui| {
                        ui.label(RichText::new(name).font(theme::medium(13.0)))
                            .on_hover_text(tr("timeline.track.volume.help"));
                        ui.label(
                            RichText::new(tr("timeline.track.volume.suffix")).color(theme.text_dim),
                        );
                    },
                    |_| {},
                );
                if field.changed {
                    actions.push(TrackAction::SetVolume(row, (percent / 100.0) as f32));
                }
            }
            let mut muted = track.muted;
            if setting_toggle(
                ui,
                tr("inspector.track.mute"),
                tr("timeline.track.mute"),
                &mut muted,
            ) {
                actions.push(TrackAction::ToggleMute(row));
            }
            let mut solo = track.solo;
            if setting_toggle(
                ui,
                tr("inspector.track.solo"),
                tr("timeline.track.solo"),
                &mut solo,
            ) {
                actions.push(TrackAction::ToggleSolo(row));
            }
            if track.kind == TrackKind::Audio {
                let mut bus = track.bus.clone();
                setting_row(
                    ui,
                    tr("inspector.track.bus"),
                    tr("timeline.track.bus.help"),
                    |ui| {
                        egui::ComboBox::from_id_salt("track-inspector-bus")
                            .width(CHOICE_WIDTH)
                            .selected_text(&bus)
                            .show_ui(ui, |ui| {
                                for name in buses {
                                    ui.selectable_value(&mut bus, name.clone(), name);
                                }
                            });
                    },
                );
                if bus != track.bus {
                    actions.push(TrackAction::SetBus(row, bus));
                }
            }
        });
    });
    actions
}
