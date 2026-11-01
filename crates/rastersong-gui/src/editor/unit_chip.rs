//! The unit on a length or rate's value box: a menu of note lengths, times, picture and signal
//! units, topped by the Free/Sync switch that rounds the value to whole units and, for a rate,
//! whether the value is cycles per unit or one cycle every so many.

use eframe::egui::{self, RichText, Ui};
use rastersong_engine::Reading;
use rastersong_lang::{tr, tr_args};

use crate::theme::Theme;

/// The tempo-locked note lengths, shortest first, each straight, dotted or a triplet.
const NOTES: [&str; 6] = ["32nd", "16th", "8th", "beat", "half", "bar"];
/// The ways a note length is played, as the suffix its unit's name takes.
const FEELS: [(&str, &str); 3] = [
    ("", "editor.unit.straight"),
    ("_dotted", "editor.unit.dotted"),
    ("_triplet", "editor.unit.triplet"),
];
/// The other units, under their group's heading.
const GROUPS: [(&str, &[&str]); 3] = [
    ("editor.unit.time", &["ms", "second", "frame"]),
    ("editor.unit.picture", &["pixel", "row"]),
    ("editor.unit.signal", &["sample"]),
];

/// The note or unit an option names, and its feel's suffix: `16th_dotted` is `16th` and
/// `_dotted`.
fn split(option: &str) -> (&str, &str) {
    match option.find('_') {
        Some(i) => option.split_at(i),
        None => (option, ""),
    }
}

/// The lang key of a unit's name, for one of it or `many`.
fn name_key(base: &str, many: bool) -> &'static str {
    match (base, many) {
        ("32nd", false) => "editor.unit.32nd.one",
        ("32nd", true) => "editor.unit.32nd.many",
        ("16th", false) => "editor.unit.16th.one",
        ("16th", true) => "editor.unit.16th.many",
        ("8th", false) => "editor.unit.8th.one",
        ("8th", true) => "editor.unit.8th.many",
        ("beat", false) => "editor.unit.beat.one",
        ("beat", true) => "editor.unit.beat.many",
        ("half", false) => "editor.unit.half.one",
        ("half", true) => "editor.unit.half.many",
        ("bar", false) => "editor.unit.bar.one",
        ("bar", true) => "editor.unit.bar.many",
        ("ms", _) => "editor.unit.ms",
        ("second", _) => "editor.unit.second",
        ("frame", false) => "editor.unit.frame.one",
        ("frame", true) => "editor.unit.frame.many",
        ("pixel", false) => "editor.unit.pixel.one",
        ("pixel", true) => "editor.unit.pixel.many",
        ("row", false) => "editor.unit.row.one",
        ("row", true) => "editor.unit.row.many",
        (_, false) => "editor.unit.sample.one",
        (_, true) => "editor.unit.sample.many",
    }
}

/// What the chip shows after `count` of `option`: "3 16ths dotted" reads as the value then this.
/// A rate read per unit reads "/ bar"; read as a period, "2 bars a cycle".
pub fn unit_label(option: &str, count: f64, rate: Option<Reading>) -> String {
    let (base, feel) = split(option);
    let per = rate == Some(Reading::Per);
    let name = tr(name_key(base, !per && count != 1.0));
    let name = match feel {
        "_dotted" => tr_args("editor.unit.feel.dotted", &[("unit", name)]),
        "_triplet" => tr_args("editor.unit.feel.triplet", &[("unit", name)]),
        _ => name.to_owned(),
    };
    match rate {
        Some(Reading::Per) => tr_args("editor.unit.per", &[("unit", &name)]),
        Some(Reading::Every) => tr_args("editor.unit.every", &[("unit", &name)]),
        None => name,
    }
}

/// The unit menu for a value of `count` (in `unit`, the option it is set to), with the value's
/// Free/Sync switch (`sync`, for a value that isn't always whole) and a rate's reading (`rate`) at
/// the top. The chip is lit while Sync is on. Returns true when the user changed the unit or the
/// reading.
pub fn unit_chip(
    ui: &mut Ui,
    id: egui::Id,
    unit: &mut String,
    count: f64,
    mut rate: Option<&mut Reading>,
    mut sync: Option<&mut bool>,
) -> bool {
    let before = (unit.clone(), rate.as_deref().copied());
    let (base, feel) = split(&before.0);
    let is_note = NOTES.contains(&base);
    let shown = unit_label(unit, count, before.1);
    let font = egui::FontId::proportional(12.0);
    let width = ui
        .painter()
        .layout_no_wrap(shown.clone(), font.clone(), egui::Color32::WHITE)
        .size()
        .x;
    let synced = sync.as_ref().is_some_and(|s| **s);
    let mut text = RichText::new(shown).font(font);
    if synced {
        text = text.color(Theme::of(ui.ctx()).accent);
    }
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(width + 2.0 * ui.spacing().icon_width)
        .height(400.0)
        .show_ui(ui, |ui| {
            ui.set_min_width(190.0);
            if let Some(sync) = sync.as_mut() {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut **sync, false, tr("editor.param.free"))
                        .on_hover_text(tr("editor.param.sync.help"));
                    ui.selectable_value(&mut **sync, true, tr("editor.param.sync"))
                        .on_hover_text(tr("editor.param.sync.help"));
                });
                ui.separator();
            }
            if let Some(reading) = rate.as_mut() {
                ui.weak(tr("editor.unit.read"));
                ui.horizontal(|ui| {
                    for (option, label) in [
                        (Reading::Per, "editor.unit.read.per"),
                        (Reading::Every, "editor.unit.read.every"),
                    ] {
                        ui.selectable_value(&mut **reading, option, tr(label))
                            .on_hover_text(tr("editor.unit.read.help"));
                    }
                });
                ui.separator();
            }
            ui.weak(tr("editor.unit.tempo"));
            ui.horizontal_wrapped(|ui| {
                for note in NOTES {
                    let option = format!("{note}{}", if is_note { feel } else { "" });
                    let name = unit_label(note, 2.0, None);
                    if ui.selectable_label(is_note && base == note, name).clicked() {
                        *unit = option;
                    }
                }
            });
            ui.add_enabled_ui(is_note, |ui| {
                ui.horizontal(|ui| {
                    for (suffix, label) in FEELS {
                        if ui
                            .selectable_label(is_note && feel == suffix, tr(label))
                            .clicked()
                        {
                            *unit = format!("{base}{suffix}");
                        }
                    }
                });
            });
            for (heading, options) in GROUPS {
                ui.separator();
                ui.weak(tr(heading));
                ui.horizontal_wrapped(|ui| {
                    for &option in options {
                        let name = unit_label(option, 2.0, None);
                        ui.selectable_value(unit, option.to_owned(), name);
                    }
                });
            }
        })
        .response
        .on_hover_text(tr("editor.unit.help"));
    (unit.clone(), rate.as_deref().copied()) != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_count_the_unit() {
        assert_eq!(unit_label("16th", 3.0, None), "16ths");
        assert_eq!(unit_label("beat", 1.0, None), "beat");
        assert_eq!(unit_label("half", 2.0, None), "halves");
        assert_eq!(unit_label("8th_dotted", 3.0, None), "8ths dotted");
        assert_eq!(unit_label("16th_triplet", 2.0, None), "16ths triplet");
        assert_eq!(unit_label("frame", 3.0, None), "frames");
    }

    #[test]
    fn rates_read_per_unit_or_as_a_period() {
        assert_eq!(unit_label("bar", 4.0, Some(Reading::Per)), "/ bar");
        assert_eq!(unit_label("bar", 2.0, Some(Reading::Every)), "bars a cycle");
        assert_eq!(
            unit_label("beat", 1.0, Some(Reading::Every)),
            "beat a cycle"
        );
    }

    #[test]
    fn every_menu_option_is_a_unit() {
        let options = rastersong_engine::Unit::OPTIONS;
        let mut offered: Vec<String> = GROUPS
            .iter()
            .flat_map(|(_, o)| o.iter().map(|o| o.to_string()))
            .collect();
        for note in NOTES {
            for (suffix, _) in FEELS {
                offered.push(format!("{note}{suffix}"));
            }
        }
        offered.sort();
        let mut all: Vec<String> = options.iter().map(|o| o.to_string()).collect();
        all.sort();
        assert_eq!(offered, all);
    }
}
