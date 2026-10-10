//! FX: graphs placed on tracks, folders, the master and items. See
//! [Routing](../../../../docs/engine.md#routing).
//!
//! An FX chain runs in order on what it sits on. Each FX's main ports (`Video`, `Audio`) read
//! what the chain is given; its other ports read the tracks its receives name, after their own
//! FX and before their volume. A port nothing fills reads zeros.

use rastersong_graph::GraphDesc;

use super::{ItemRef, Project};
use crate::routing::{RouteTrack, Routing, receive_ports};
use crate::timeline::Fx;

/// What an FX chain sits on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FxTarget {
    /// The master: everything the tracks send to the top.
    Master,
    /// A track or folder, by name.
    Track(String),
    Item(ItemRef),
}

impl Project {
    /// The description of graph `id`, open or stored.
    pub fn graph_desc(&self, id: u32) -> Option<&GraphDesc> {
        if id == self.graph_id {
            Some(&self.graph)
        } else {
            self.graphs.iter().find(|g| g.id == id).map(|g| &g.graph)
        }
    }

    /// Every FX chain, with what it sits on: the master's, then each track's, then each item's.
    pub fn fx_chains(&self) -> impl Iterator<Item = (FxTarget, &[Fx])> {
        let master = std::iter::once((FxTarget::Master, &self.master_fx[..]));
        let tracks = self
            .tracks
            .iter()
            .map(|t| (FxTarget::Track(t.name.clone()), &t.fx[..]));
        let items = self.tracks.iter().flat_map(|t| {
            t.items.iter().enumerate().map(|(i, item)| {
                (
                    FxTarget::Item(ItemRef::new(t.name.clone(), i)),
                    &item.fx[..],
                )
            })
        });
        master.chain(tracks).chain(items)
    }

    /// Whether any FX is on. Without one the output is the plain track mix.
    pub fn has_fx(&self) -> bool {
        self.fx_chains()
            .any(|(_, chain)| chain.iter().any(|f| !f.bypass))
    }

    /// Whether the preview skips every FX and plays the track mix: FX are bypassed, or none is
    /// on.
    pub fn plays_track_mix(&self) -> bool {
        self.bypass_graph || !self.has_fx()
    }

    /// The FX chain on `target`, if it exists.
    pub fn fx(&self, target: &FxTarget) -> Option<&[Fx]> {
        match target {
            FxTarget::Master => Some(&self.master_fx),
            FxTarget::Track(name) => self.track(name).map(|t| &t.fx[..]),
            FxTarget::Item(item) => self
                .track(&item.track)
                .and_then(|t| t.items.get(item.item))
                .map(|i| &i.fx[..]),
        }
    }

    fn fx_mut(&mut self, target: &FxTarget) -> Option<&mut Vec<Fx>> {
        match target {
            FxTarget::Master => Some(&mut self.master_fx),
            FxTarget::Track(name) => self
                .tracks
                .iter_mut()
                .find(|t| t.name == *name)
                .map(|t| &mut t.fx),
            FxTarget::Item(item) => self
                .tracks
                .iter_mut()
                .find(|t| t.name == item.track)
                .and_then(|t| t.items.get_mut(item.item))
                .map(|i| &mut i.fx),
        }
    }

    /// Adds graph `graph` to the end of `target`'s chain, its ports named after tracks receiving
    /// from them. Returns its place in the chain, or `None` for a target or graph that doesn't
    /// exist.
    pub fn add_fx(&mut self, target: &FxTarget, graph: u32) -> Option<usize> {
        let receives = receive_ports(self.graph_desc(graph)?)
            .into_iter()
            .filter(|port| self.has_track(port))
            .map(|port| (port.clone(), port))
            .collect();
        let chain = self.fx_mut(target)?;
        chain.push(Fx {
            receives,
            ..Fx::new(graph)
        });
        Some(chain.len() - 1)
    }

    /// Removes FX `index` of `target`'s chain.
    pub fn remove_fx(&mut self, target: &FxTarget, index: usize) -> bool {
        match self.fx_mut(target) {
            Some(chain) if index < chain.len() => {
                chain.remove(index);
                true
            }
            _ => false,
        }
    }

    /// Moves FX `from` of `target`'s chain to place `to`.
    pub fn move_fx(&mut self, target: &FxTarget, from: usize, to: usize) -> bool {
        match self.fx_mut(target) {
            Some(chain) if from < chain.len() && to < chain.len() => {
                let fx = chain.remove(from);
                chain.insert(to, fx);
                true
            }
            _ => false,
        }
    }

    /// Bypasses FX `index` of `target`'s chain, or turns it back on.
    pub fn set_fx_bypass(&mut self, target: &FxTarget, index: usize, bypass: bool) {
        if let Some(fx) = self.fx_mut(target).and_then(|c| c.get_mut(index)) {
            fx.bypass = bypass;
        }
    }

    /// Fills port `port` of FX `index` of `target`'s chain from track `track`, or from nothing
    /// (zeros) for `None`.
    pub fn set_fx_receive(
        &mut self,
        target: &FxTarget,
        index: usize,
        port: &str,
        track: Option<&str>,
    ) {
        let Some(fx) = self.fx_mut(target).and_then(|c| c.get_mut(index)) else {
            return;
        };
        match track {
            Some(track) => {
                fx.receives.insert(port.to_owned(), track.to_owned());
            }
            None => {
                fx.receives.remove(port);
            }
        }
    }

    /// Removes every FX that uses graph `graph`; used when the graph is deleted.
    pub fn remove_graph_fx(&mut self, graph: u32) {
        self.master_fx.retain(|f| f.graph != graph);
        for track in &mut self.tracks {
            track.fx.retain(|f| f.graph != graph);
            for item in &mut track.items {
                item.fx.retain(|f| f.graph != graph);
            }
        }
    }

    /// The receives naming tracks that no longer exist: (chain, FX, port).
    pub fn dangling_receives(&self) -> Vec<(FxTarget, usize, String)> {
        let mut found = Vec::new();
        for (target, chain) in self.fx_chains() {
            for (i, fx) in chain.iter().enumerate() {
                for (port, track) in &fx.receives {
                    if !self.has_track(track) {
                        found.push((target.clone(), i, port.clone()));
                    }
                }
            }
        }
        found
    }

    /// Points every receive from track `old` at `new`.
    pub(super) fn rename_receives(&mut self, old: &str, new: &str) {
        let rename = |chain: &mut Vec<Fx>| {
            for track in chain.iter_mut().flat_map(|f| f.receives.values_mut()) {
                if track == old {
                    *track = new.to_owned();
                }
            }
        };
        rename(&mut self.master_fx);
        for track in &mut self.tracks {
            rename(&mut track.fx);
            for item in &mut track.items {
                rename(&mut item.fx);
            }
        }
    }

    /// What the engine routes: the track tree with each track's FX, the master's FX, and the
    /// stored graphs. Solos leave out the other tracks of their kind; folders are never left
    /// out. Items' FX come with the items.
    pub fn routing(&self) -> Routing {
        let soloing = |kind| self.soloing(kind);
        Routing {
            tracks: self
                .tracks
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let kind = if t.folder { None } else { self.track_kind(t) };
                    RouteTrack {
                        name: t.name.clone(),
                        depth: t.depth,
                        folder: t.folder,
                        kind,
                        fx: t.fx.clone(),
                        volume: t.volume,
                        muted: t.muted,
                        master_send: t.master_send,
                        solo_out: kind.is_some_and(|k| soloing(k) && !self.soloed(i)),
                        bus: t.bus.clone(),
                    }
                })
                .collect(),
            master_fx: self.master_fx.clone(),
            graphs: self
                .graphs
                .iter()
                .map(|g| (g.id, g.graph.clone()))
                .collect(),
            open: self.graph_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResourceKind;
    use crate::project::ProjectTrack;
    use crate::timeline::TrackKind;

    fn ports_graph() -> GraphDesc {
        GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [
                { "id": "v", "type": "video_input" },
                { "id": "k", "type": "audio_input", "params": { "port": "Kick" } },
                { "id": "o", "type": "output" } ],
                "connections": [ { "from": "v", "to": "o" } ] }"#,
        )
        .unwrap()
    }

    fn project() -> Project {
        let mut project = Project::new(ports_graph());
        let audio = project.add_resource(ResourceKind::Audio, "kick", "kick.wav", None);
        let video = project.add_resource(ResourceKind::Video, "clip", "clip.mp4", None);
        project.tracks.push(ProjectTrack::new("Kick".into(), audio));
        project.tracks.push(ProjectTrack::new("Clip".into(), video));
        project
    }

    #[test]
    fn added_fx_receive_from_the_tracks_their_ports_are_named_after() {
        let mut project = project();
        assert!(!project.has_fx());
        let target = FxTarget::Track("Clip".into());
        assert_eq!(project.add_fx(&target, project.graph_id), Some(0));
        let fx = &project.fx(&target).unwrap()[0];
        assert_eq!(fx.receives.get("Kick").map(String::as_str), Some("Kick"));
        assert!(project.has_fx());
        assert!(!project.plays_track_mix());

        project.set_fx_bypass(&target, 0, true);
        assert!(project.plays_track_mix());
        assert_eq!(project.add_fx(&FxTarget::Track("Nope".into()), 0), None);
        assert_eq!(project.add_fx(&target, 99), None);
    }

    #[test]
    fn renaming_a_track_renames_the_receives_from_it() {
        let mut project = project();
        let item = FxTarget::Item(ItemRef::new("Clip", 0));
        project.add_fx(&item, project.graph_id).unwrap();
        project.add_fx(&FxTarget::Master, project.graph_id).unwrap();
        assert!(project.rename_track("Kick", "Drum"));
        for target in [item, FxTarget::Master] {
            let fx = &project.fx(&target).unwrap()[0];
            assert_eq!(fx.receives.get("Kick").map(String::as_str), Some("Drum"));
        }
        assert!(project.dangling_receives().is_empty());
        project.set_fx_receive(&FxTarget::Master, 0, "Kick", Some("Gone"));
        assert_eq!(
            project.dangling_receives(),
            [(FxTarget::Master, 0, "Kick".to_owned())]
        );
    }

    #[test]
    fn chains_reorder_and_lose_a_deleted_graph() {
        let mut project = project();
        let target = FxTarget::Master;
        project.add_fx(&target, project.graph_id).unwrap();
        project.master_fx.push(Fx::new(7));
        assert!(project.move_fx(&target, 1, 0));
        assert_eq!(project.master_fx[0].graph, 7);
        assert!(!project.move_fx(&target, 0, 2));
        project.remove_graph_fx(7);
        assert_eq!(project.master_fx.len(), 1);
        assert!(project.remove_fx(&target, 0));
        assert!(!project.remove_fx(&target, 0));
    }

    #[test]
    fn the_routing_leaves_out_what_a_solo_leaves_out_but_never_folders() {
        let mut project = project();
        let folder = project.add_folder("Bus");
        let i = project.track_index(&folder).unwrap();
        project.tracks[i].solo = true;
        project.tracks[0].solo = true;
        let routing = project.routing();
        let out: Vec<_> = routing
            .tracks
            .iter()
            .map(|t| (t.name.as_str(), t.kind, t.solo_out))
            .collect();
        assert_eq!(
            out,
            [
                ("Kick", Some(TrackKind::Audio), false),
                ("Clip", Some(TrackKind::Video), false),
                ("Bus", None, false),
            ]
        );
        project.tracks[0].solo = false;
        let kick = project.tracks[0].resource.unwrap();
        project.tracks.push(ProjectTrack::new("Snare".into(), kick));
        project.tracks[0].solo = true;
        let routing = project.routing();
        assert!(
            routing
                .tracks
                .iter()
                .any(|t| t.name == "Snare" && t.solo_out)
        );
        assert_eq!(routing.open, project.graph_id);
    }
}
