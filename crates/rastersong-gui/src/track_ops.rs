//! Reordering the project's audio tracks, as the timeline asks. A plain function on the track
//! list, so it can be tested without a window.

use rastersong_engine::ProjectTrack;

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
    use rastersong_engine::ResourceId;

    use super::*;

    fn tracks(count: usize) -> Vec<ProjectTrack> {
        (0..count)
            .map(|i| ProjectTrack::new(format!("t{i}"), ResourceId(1)))
            .collect()
    }

    #[test]
    fn moving_a_track_keeps_the_selection_on_the_same_track() {
        let names = |l: &[ProjectTrack]| l.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
        let mut list = tracks(4);
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
