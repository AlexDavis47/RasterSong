//! Edits to the project's audio tracks that the timeline asks for: solo and reordering. Plain
//! functions on the track list, so they can be tested without a window.

use rastersong_engine::ProjectTrack;

/// What a solo remembers, to put the mutes back when the track is un-soloed.
#[derive(Debug, Clone, PartialEq)]
pub struct SoloState {
    /// The soloed track.
    pub track: String,
    /// Every track's name and mute before the solo.
    pub previous: Vec<(String, bool)>,
}

/// Alt+click on the mute button of track `index`: mutes every other track and unmutes this one,
/// or, if it is already the one playing alone, puts the mutes back as they were.
pub fn toggle_solo(tracks: &mut [ProjectTrack], index: usize, state: &mut Option<SoloState>) {
    let Some(name) = tracks.get(index).map(|t| t.name.clone()) else {
        return;
    };
    let alone = tracks
        .iter()
        .enumerate()
        .all(|(j, t)| t.muted == (j != index));
    match state.take() {
        Some(solo) if alone && solo.track == name => {
            for track in tracks.iter_mut() {
                if let Some(&(_, muted)) = solo.previous.iter().find(|(n, _)| *n == track.name) {
                    track.muted = muted;
                }
            }
        }
        _ => {
            *state = Some(SoloState {
                track: name,
                previous: tracks.iter().map(|t| (t.name.clone(), t.muted)).collect(),
            });
            for (j, track) in tracks.iter_mut().enumerate() {
                track.muted = j != index;
            }
        }
    }
}

/// Moves the track at `from` to index `to`. Returns where the selected track ends up, given where
/// it was.
pub fn move_track(
    tracks: &mut Vec<ProjectTrack>,
    from: usize,
    to: usize,
    selected: Option<usize>,
) -> Option<usize> {
    if from >= tracks.len() || to >= tracks.len() || from == to {
        return selected;
    }
    let track = tracks.remove(from);
    tracks.insert(to, track);
    selected.map(|s| {
        if s == from {
            to
        } else if from < s && s <= to {
            s - 1
        } else if to <= s && s < from {
            s + 1
        } else {
            s
        }
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn tracks(muted: &[bool]) -> Vec<ProjectTrack> {
        muted
            .iter()
            .enumerate()
            .map(|(i, &m)| ProjectTrack {
                muted: m,
                ..ProjectTrack::new(format!("t{i}"), PathBuf::from("x.wav"))
            })
            .collect()
    }

    fn mutes(tracks: &[ProjectTrack]) -> Vec<bool> {
        tracks.iter().map(|t| t.muted).collect()
    }

    #[test]
    fn solo_mutes_the_others_and_a_second_alt_click_restores_them() {
        let mut list = tracks(&[false, true, false]);
        let mut state = None;
        toggle_solo(&mut list, 0, &mut state);
        assert_eq!(mutes(&list), [false, true, true]);
        toggle_solo(&mut list, 0, &mut state);
        assert_eq!(mutes(&list), [false, true, false], "the old mutes are back");
        assert!(state.is_none());
    }

    #[test]
    fn soloing_another_track_switches_the_solo_and_keeps_the_original_mutes() {
        let mut list = tracks(&[false, true, false]);
        let mut state = None;
        toggle_solo(&mut list, 0, &mut state);
        toggle_solo(&mut list, 2, &mut state);
        assert_eq!(mutes(&list), [true, true, false]);
        toggle_solo(&mut list, 2, &mut state);
        // Restores what the second solo remembered: the state after the first.
        assert_eq!(mutes(&list), [false, true, true]);
    }

    #[test]
    fn an_unmuted_lone_track_without_a_solo_is_soloed_not_restored() {
        let mut list = tracks(&[true, false]);
        let mut state = None;
        toggle_solo(&mut list, 1, &mut state);
        assert_eq!(mutes(&list), [true, false]);
        assert!(state.is_some());
        // And a manual mute change in between makes the next Alt+click a new solo.
        list[0].muted = false;
        toggle_solo(&mut list, 1, &mut state);
        assert_eq!(mutes(&list), [true, false]);
    }

    #[test]
    fn moving_a_track_keeps_the_selection_on_the_same_track() {
        let names = |l: &[ProjectTrack]| l.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
        let mut list = tracks(&[false; 4]);
        let selected = move_track(&mut list, 0, 2, Some(0));
        assert_eq!(names(&list), ["t1", "t2", "t0", "t3"]);
        assert_eq!(selected, Some(2));
        assert_eq!(move_track(&mut list, 0, 2, Some(1)), Some(0));
        assert_eq!(names(&list), ["t2", "t0", "t1", "t3"]);
        assert_eq!(move_track(&mut list, 3, 1, Some(2)), Some(3));
        assert_eq!(move_track(&mut list, 3, 1, None), None);
        // Out of range or no move: unchanged.
        assert_eq!(move_track(&mut list, 9, 0, Some(1)), Some(1));
    }
}
