//! What the renderer composes: every track (the graph tracks above the video and audio) and the
//! resources their items play. See [Composition](../../../docs/engine.md#composition).

use std::collections::BTreeMap;

use rastersong_graph::nodes::{
    AUDIO_INPUT, BELOW, NAME_PARAM, NAMED_INPUT, SOURCE_PARAM, TRACK_PARAM, VIDEO_INPUT, named_key,
    source_key,
};
use rastersong_graph::{GraphDesc, ParamValue};

use crate::project::{MediaFile, ResourceContent, ResourceId};
use crate::timeline::{Bus, Item, Source, Timebase, TrackKind};

/// Whether an input node reads pictures or sound, or (a Named Input) whatever was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Video,
    Audio,
    Named,
}

/// What one input node of a graph reads, and the name the graph is filled by for it. Several
/// nodes reading the same source share it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPort {
    /// The name the host fills: `Video` or `Audio` for the main inputs.
    pub key: String,
    pub kind: InputKind,
    pub source: Source,
}

/// What a node of type `kind` with `params` reads; `None` for nodes other than inputs.
pub fn port_of(kind: &str, params: &BTreeMap<String, ParamValue>) -> Option<InputPort> {
    let text = |name: &str| match params.get(name) {
        Some(ParamValue::Text(text)) => text.as_str(),
        _ => "",
    };
    let (kind, key, source) = match kind {
        VIDEO_INPUT | AUDIO_INPUT => {
            let video = kind == VIDEO_INPUT;
            // A missing setting is the default: what is below.
            let source = params
                .get(SOURCE_PARAM)
                .map_or(BELOW, |_| text(SOURCE_PARAM));
            (
                if video {
                    InputKind::Video
                } else {
                    InputKind::Audio
                },
                source_key(video, source),
                Source::from_setting(source),
            )
        }
        NAMED_INPUT => {
            let (track, name) = (text(TRACK_PARAM), text(NAME_PARAM));
            let source = if track.is_empty() || name.is_empty() {
                Source::Nothing
            } else {
                Source::Named {
                    track: track.to_owned(),
                    name: name.to_owned(),
                }
            };
            (InputKind::Named, named_key(track, name), source)
        }
        _ => return None,
    };
    Some(InputPort { key, kind, source })
}

/// What graph `desc`'s input nodes read, each once, in the order their first nodes come.
pub fn input_ports(desc: &GraphDesc) -> Vec<InputPort> {
    let mut ports: Vec<InputPort> = Vec::new();
    for port in desc
        .nodes
        .iter()
        .filter_map(|n| port_of(&n.kind, &n.params))
    {
        if !ports.iter().any(|p| p.key == port.key) {
            ports.push(port);
        }
    }
    ports
}

/// A track as the renderer composes it: graph, video and audio tracks alike.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposedTrack {
    /// The name sources select it by.
    pub name: String,
    pub kind: TrackKind,
    /// Its items, muted ones included: a muted item reads as a gap.
    pub items: Vec<Item>,
    /// Whether it is in the mix: not muted, and not left out by a solo on another track of its
    /// kind. Graphs read a video or audio track's stem either way.
    pub in_mix: bool,
    /// Its level in the audio mix, for kinds that [have one](TrackKind::has_level).
    pub gain: f32,
    /// The bus an audio track is summed into.
    pub bus: String,
}

impl ComposedTrack {
    /// A track of `kind` holding `items`, in the mix at full level on the master bus.
    pub fn new(name: impl Into<String>, kind: TrackKind, items: Vec<Item>) -> Self {
        Self {
            name: name.into(),
            kind,
            items,
            in_mix: true,
            gain: 1.0,
            bus: rastersong_graph::nodes::DEFAULT_BUS.to_owned(),
        }
    }

    /// Its level in the mix: its gain where its kind has a level (all or nothing otherwise), or
    /// 0 when it is out of the mix.
    pub fn mix_gain(&self) -> f32 {
        match (self.in_mix, self.kind.has_level()) {
            (false, _) => 0.0,
            (true, false) => 1.0,
            (true, true) => self.gain,
        }
    }
}

/// A resource the composition's items may play.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposedResource {
    pub id: ResourceId,
    /// What errors call it.
    pub name: String,
    pub content: ResourceContent,
}

impl ComposedResource {
    pub fn kind(&self) -> TrackKind {
        self.content.kind()
    }

    /// The file a video or audio resource reads.
    pub fn file(&self) -> Option<&MediaFile> {
        self.content.file()
    }

    /// A graph resource's graph.
    pub fn graph(&self) -> Option<&GraphDesc> {
        self.content.graph()
    }
}

/// What the renderer composes: the project's timebase, buses, tracks and the resources their
/// items play. See [Composition](../../../docs/engine.md#composition).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Composition {
    /// The project's timebase. `None` takes it from the first video track, or
    /// [`Timebase::DEFAULT`] when there is none.
    pub timebase: Option<Timebase>,
    /// The output buses, master first. Empty means just [`Bus::main`].
    pub buses: Vec<Bus>,
    /// Every track, top first: the graph tracks, then the video tracks, then the audio tracks.
    /// Over the mix, the topmost graph track with an item playing wins.
    pub tracks: Vec<ComposedTrack>,
    /// Every resource an item may play. An item of a resource not listed (a file that is
    /// missing) reads as a gap.
    pub resources: Vec<ComposedResource>,
}

impl Composition {
    /// The master bus: the first, which the preview plays and the export writes.
    pub fn master(&self) -> Bus {
        self.buses
            .first()
            .cloned()
            .map_or_else(Bus::main, |bus| Bus {
                master: true,
                ..bus
            })
    }

    /// The buses that can be rendered: the composition's, or the stereo main bus when it lists
    /// none.
    pub fn render_buses(&self) -> Vec<Bus> {
        if self.buses.is_empty() {
            vec![Bus::main()]
        } else {
            let mut buses = self.buses.clone();
            Bus::mark_master(&mut buses);
            buses
        }
    }

    /// The resource `id`, if the composition has it.
    pub fn resource(&self, id: ResourceId) -> Option<&ComposedResource> {
        self.resources.iter().find(|r| r.id == id)
    }

    /// The resource a video or audio track plays: its first item's, while each such track plays
    /// one resource.
    pub fn track_resource(&self, track: &ComposedTrack) -> Option<&ComposedResource> {
        self.resource(track.items.first()?.resource)
    }

    /// The tracks of `kind`, top first.
    pub fn tracks_of(&self, kind: TrackKind) -> impl Iterator<Item = &ComposedTrack> {
        self.tracks.iter().filter(move |t| t.kind == kind)
    }

    /// The audio tracks summed into `bus` in the track mix.
    pub fn tracks_on<'a>(&'a self, bus: &'a str) -> impl Iterator<Item = &'a ComposedTrack> {
        self.tracks_of(TrackKind::Audio)
            .filter(move |t| t.bus == bus)
    }

    /// The items that can play: those of a resource the composition has.
    fn playable(&self) -> impl Iterator<Item = &Item> {
        self.tracks
            .iter()
            .flat_map(|t| &t.items)
            .filter(|i| self.resource(i.resource).is_some())
    }

    /// Whether nothing can play: no item of a resource the composition has.
    pub fn is_empty(&self) -> bool {
        self.playable().next().is_none()
    }

    /// Where the last item that can play ends, in seconds (0 with none), each file's resource
    /// lasting `duration(id)` seconds (none when it isn't known). A graph item lasts as long as it
    /// is placed for.
    pub fn end(&self, duration: impl Fn(ResourceId) -> Option<f64>) -> f64 {
        self.playable()
            .map(|i| i.timeline_end(duration(i.resource).unwrap_or(0.0)))
            .fold(0.0, f64::max)
    }

    /// Whether a graph item plays: one of a graph the composition has, not muted, on a graph
    /// track in the mix.
    pub fn has_graph_items(&self) -> bool {
        self.tracks_of(TrackKind::Graph)
            .filter(|t| t.in_mix)
            .flat_map(|t| &t.items)
            .any(|i| !i.muted && self.resource(i.resource).is_some())
    }

    /// The composition with every graph track left out: the plain mix, as a render with every
    /// graph bypassed shows and plays it.
    pub fn bypassed(mut self) -> Self {
        self.tracks.retain(|t| t.kind != TrackKind::Graph);
        self.resources.retain(|r| r.kind() != TrackKind::Graph);
        self
    }

    /// Whether the two render the same frames and graph sound. With no graph item playing, the
    /// audio tracks' levels and routing and the buses other than the master only shape the track
    /// mix's sound, which is played, not rendered (muting or soloing a video track still changes
    /// the picture). With graph items playing, graphs hear the mix, so everything counts.
    pub fn renders_like(&self, other: &Self) -> bool {
        if self.has_graph_items() || other.has_graph_items() {
            return self == other;
        }
        self.timebase == other.timebase
            && self.master() == other.master()
            && self.resources == other.resources
            && self.tracks.len() == other.tracks.len()
            && self.tracks.iter().zip(&other.tracks).all(|(a, b)| {
                (&a.name, a.kind, &a.items) == (&b.name, b.kind, &b.items)
                    && (a.kind == TrackKind::Audio || a.in_mix == b.in_mix)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_read_what_their_source_names() {
        let desc = GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [
                { "id": "v", "type": "video_input" },
                { "id": "a", "type": "audio_input", "params": { "source": "below" } },
                { "id": "k", "type": "audio_input", "params": { "source": "track:Kick" } },
                { "id": "k2", "type": "audio_input", "params": { "source": "track:Kick" } },
                { "id": "kv", "type": "video_input", "params": { "source": "track:Kick" } },
                { "id": "g", "type": "video_input", "params": { "source": "graph:FX" } },
                { "id": "mask", "type": "named_input", "params": { "track": "FX", "name": "Mask" } },
                { "id": "m", "type": "audio_input", "params": { "source": "mix" } },
                { "id": "n", "type": "video_input", "params": { "source": "" } },
                { "id": "o", "type": "output" } ] }"#,
        )
        .unwrap();
        let sources: Vec<(InputKind, Source)> = input_ports(&desc)
            .into_iter()
            .map(|p| (p.kind, p.source))
            .collect();
        assert_eq!(
            sources,
            [
                (InputKind::Video, Source::Below),
                (InputKind::Audio, Source::Below),
                (InputKind::Audio, Source::Track("Kick".into())),
                (InputKind::Video, Source::Track("Kick".into())),
                (InputKind::Video, Source::Graph("FX".into())),
                (
                    InputKind::Named,
                    Source::Named {
                        track: "FX".into(),
                        name: "Mask".into()
                    }
                ),
                (InputKind::Audio, Source::Mix),
                (InputKind::Video, Source::Nothing),
            ]
        );
    }

    #[test]
    fn a_composition_differing_only_in_audio_levels_renders_like() {
        use crate::{GraphDesc, ItemLengths, Project};

        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        project.add_track(TrackKind::Video, "v", "v.mp4");
        project.add_track(TrackKind::Audio, "a", "a.wav");
        let before = project.composition();
        // Levels and routing only shape the track mix's sound.
        project.tracks[1].volume = 0.5;
        project.tracks[1].muted = true;
        project.tracks[1].bus = "Other".into();
        assert!(project.composition().renders_like(&before));
        assert_ne!(project.composition(), before);
        // Muting the video changes the picture.
        project.tracks[0].muted = true;
        assert!(!project.composition().renders_like(&before));
        project.tracks[0].muted = false;

        // With a graph item playing, graphs hear the mix: levels count.
        let lengths = ItemLengths {
            new_graph_length: 1.0,
            ..ItemLengths::default()
        };
        project.add_track_for(Project::FIRST_GRAPH, 0.0, &lengths);
        let before = project.composition();
        let a = project.track_index("a").unwrap();
        project.tracks[a].volume = 0.25;
        assert!(!project.composition().renders_like(&before));
    }

    #[test]
    fn source_settings_round_trip() {
        for source in [
            Source::Below,
            Source::Mix,
            Source::Track("a:b".into()),
            Source::Graph("FX".into()),
            Source::Nothing,
        ] {
            assert_eq!(Source::from_setting(&source.setting()), source);
        }
    }
}
