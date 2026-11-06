//! Composition: the tracks' mix and the graph tracks above it, each graph's input nodes reading
//! the sources they name, rendered by the sequential renderer, the service and the offline render.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{FRAMES, backend, wait_until};
use rastersong_engine::{
    AudioResources, AudioSink, Bus, ComposedResource, ComposedTrack, Composition, Engine,
    EngineConfig, EngineError, EngineStatus, FakeVideo, FrameSink, GraphDesc, Item, MediaFile,
    OutputSize, Rational, Registry, RenderInfo, RenderSettings, RenderedFrame, Renderer,
    ResourceContent, ResourceId, TrackKind, render,
};

const FPS: f64 = 30.0;

fn graph(nodes: &str, connections: &str) -> String {
    format!(r#"{{ "version": 0, "nodes": [ {nodes} ], "connections": [ {connections} ] }}"#)
}

fn passthrough() -> String {
    graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "o" }"#,
    )
}

/// A Video Input reading `source`, or with an `output` name, a Named Input reading that Named
/// Output of the graph track `source` names (`graph:<track>`), as a node.
fn source_node(id: &str, source: &str, output: &str) -> String {
    match source.strip_prefix("graph:").filter(|_| !output.is_empty()) {
        Some(track) => format!(
            r#"{{ "id": "{id}", "type": "named_input", "params": {{ "track": "{track}", "name": "{output}" }} }}"#
        ),
        None => format!(
            r#"{{ "id": "{id}", "type": "video_input", "params": {{ "source": "{source}" }} }}"#
        ),
    }
}

/// The picture of `source` straight to the output.
fn show(source: &str, output: &str) -> String {
    graph(
        &format!(
            r#"{}, {{ "id": "o", "type": "output" }}"#,
            source_node("v", source, output)
        ),
        r#"{ "from": "v", "to": "o" }"#,
    )
}

fn invert() -> String {
    graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "i", "type": "invert" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "i" }, { "from": "i", "to": "o" }"#,
    )
}

fn crush() -> String {
    graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "c", "type": "bitcrush", "params": { "bits": 2 } }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "c" }, { "from": "c", "to": "o" }"#,
    )
}

/// The picture delayed by `frames` frames.
fn delay(frames: u32) -> String {
    graph(
        &format!(
            r#"{{ "id": "v", "type": "video_input" }}, {{ "id": "d", "type": "delay", "params": {{ "time": {frames}, "time.unit": "frame", "mix": 1 }} }}, {{ "id": "o", "type": "output" }}"#
        ),
        r#"{ "from": "v", "to": "d" }, { "from": "d", "to": "o" }"#,
    )
}

/// The picture as it is, a part of a frame late: rounded up, one frame of latency.
fn shifted() -> String {
    graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "s", "type": "frequency_shifter", "params": { "shift": 0, "mix": 0 } }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "s" }, { "from": "s", "to": "o" }"#,
    )
}

/// Graph `graph` placed over frames `from..to`.
fn span(from: usize, to: usize, graph: u32) -> Item {
    Item::graph(
        ResourceId(graph),
        from as f64 / FPS,
        (to - from) as f64 / FPS,
    )
}

/// `item` from `start` seconds into its graph's own time, lasting as long.
fn from(item: Item, start: f64) -> Item {
    let length = item.end.unwrap() - item.start;
    Item {
        start,
        end: Some(start + length),
        ..item
    }
}

/// A graph track called `name` holding `items`.
fn graph_track(name: &str, items: Vec<Item>) -> ComposedTrack {
    ComposedTrack::new(name, TrackKind::Graph, items)
}

/// The clip the backend plays as [`common::VIDEO`].
const CLIP: ResourceId = ResourceId(100);
/// The other clip the backend plays.
const OTHER: ResourceId = ResourceId(101);
/// The song the backend plays as [`common::AUDIO`].
const SONG: ResourceId = ResourceId(102);
/// A clip a third of a second long.
const SHORT: ResourceId = ResourceId(103);
/// A quiet stereo sound at 48 kHz: 0.1 on the left, -0.1 on the right.
const QUIET: ResourceId = ResourceId(104);

/// A video or audio resource reading `path`.
fn file(id: ResourceId, kind: TrackKind, path: &str) -> ComposedResource {
    let file = MediaFile {
        path: PathBuf::from(path),
        stream: None,
    };
    ComposedResource {
        id,
        name: path.to_owned(),
        content: match kind {
            TrackKind::Video => ResourceContent::Video(file),
            _ => ResourceContent::Audio(file),
        },
    }
}

/// `tracks` (in the mix), no graph tracks, the backend's files, and `graphs` to place.
fn composition(tracks: &[ComposedTrack], graphs: &[(u32, String)]) -> Composition {
    let files = [
        file(CLIP, TrackKind::Video, common::VIDEO),
        file(OTHER, TrackKind::Video, "other"),
        file(SONG, TrackKind::Audio, common::AUDIO),
        file(SHORT, TrackKind::Video, "short"),
        file(QUIET, TrackKind::Audio, "quiet"),
    ];
    let graphs = graphs.iter().map(|(id, json)| ComposedResource {
        id: ResourceId(*id),
        name: format!("graph {id}"),
        content: ResourceContent::Graph {
            graph: GraphDesc::from_json(json).unwrap(),
        },
    });
    Composition {
        tracks: tracks.to_vec(),
        resources: files.into_iter().chain(graphs).collect(),
        ..Composition::default()
    }
}

/// Replaces the graph tracks of `comp` with `graph_tracks`, top first.
fn set_graph_tracks(comp: &mut Composition, graph_tracks: Vec<ComposedTrack>) {
    comp.tracks.retain(|t| t.kind != TrackKind::Graph);
    comp.tracks.splice(0..0, graph_tracks);
}

/// Adds `track` below the graph tracks of `comp`.
fn push_graph_track(comp: &mut Composition, track: ComposedTrack) {
    let at = comp.tracks_of(TrackKind::Graph).count();
    comp.tracks.insert(at, track);
}

/// The track of `comp` called `name`.
fn track<'a>(comp: &'a mut Composition, name: &str) -> &'a mut ComposedTrack {
    comp.tracks.iter_mut().find(|t| t.name == name).unwrap()
}

/// The graph of resource `id` in `comp`.
fn graph_of(comp: &mut Composition, id: u32) -> &mut GraphDesc {
    let resource = comp
        .resources
        .iter_mut()
        .find(|r| r.id == ResourceId(id))
        .unwrap();
    match &mut resource.content {
        ResourceContent::Graph { graph } => graph,
        _ => panic!("not a graph"),
    }
}

fn quiet() -> rastersong_engine::AudioClip {
    rastersong_engine::AudioClip {
        sample_rate: 48_000,
        channels: 2,
        samples: [0.1, -0.1].repeat(48_000 * 2).into(),
    }
}

/// The song and the quiet sound, decoded.
fn audio() -> AudioResources {
    use rastersong_engine::sources::Modulator;
    AudioResources::from([
        (SONG, Arc::new(Modulator::new(&common::audio()))),
        (QUIET, Arc::new(Modulator::new(&quiet()))),
    ])
}

fn build(composition: &Composition) -> Result<Renderer, EngineError> {
    Renderer::new(
        &other_backend(),
        composition,
        &audio(),
        None,
        Default::default(),
        &Bus::main(),
        &Registry::default(),
        OutputSize::Native,
    )
}

fn renderer_for(composition: &Composition) -> Renderer {
    build(composition).unwrap()
}

fn other_backend() -> rastersong_engine::FakeBackend {
    let video = |frame_count| FakeVideo {
        width: 16,
        height: 8,
        frame_count,
        frame_rate: Rational::new(30, 1),
    };
    backend()
        .with_video("other", video(FRAMES))
        .with_video("short", video(10))
}

/// The clip as the video track `Video`, followed by `tracks`.
fn with_video(tracks: Vec<ComposedTrack>) -> Vec<ComposedTrack> {
    std::iter::once(ComposedTrack::new(
        "Video",
        TrackKind::Video,
        vec![Item::whole(CLIP, 0.0)],
    ))
    .chain(tracks)
    .collect()
}

fn video_track() -> Vec<ComposedTrack> {
    with_video(Vec::new())
}

/// A video track `name` playing the `other` clip 2 frames late.
fn late(name: &str) -> ComposedTrack {
    ComposedTrack::new(name, TrackKind::Video, vec![Item::whole(OTHER, 2.0 / FPS)])
}

fn all_frames(r: &mut Renderer, frames: usize) -> Vec<Vec<u8>> {
    (0..frames)
        .map(|i| r.render(i, &|| false).unwrap().unwrap().to_vec())
        .collect()
}

/// The video track under graph `json`, placed over the whole clip.
fn over_the_video(json: &str) -> Composition {
    let mut comp = composition(&video_track(), &[(1, json.to_owned())]);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![span(0, FRAMES, 1)])]);
    comp
}

/// A graph rendered over the video track for the whole timeline.
fn reference(json: &str, frames: usize) -> Vec<Vec<u8>> {
    all_frames(&mut renderer_for(&over_the_video(json)), frames)
}

#[test]
fn without_graph_items_the_mix_shows() {
    let tracks = video_track();
    let mut r = renderer_for(&composition(&tracks, &[]));
    assert_eq!(r.latency_frames(), 0);
    assert!(matches!(r.audio_sink(), AudioSink::TrackMix));
    assert_eq!(all_frames(&mut r, 8), reference(&passthrough(), 8));
}

/// A beat's phase as the picture: it rises over each beat (15 frames at 120 bpm).
fn beat() -> String {
    graph(
        r#"{ "id": "b", "type": "beat", "params": { "shape": "phase" } }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "b", "to": "o" }"#,
    )
}

#[test]
fn an_item_starting_mid_timeline_keeps_the_projects_beat_grid() {
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, beat())]);
    // Five frames in, a third of a beat, and trimmed by two frames at its start.
    let item = from(span(5, 20, 1), 2.0 / FPS);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![item])]);
    let rendered = all_frames(&mut renderer_for(&comp), 20);
    let whole = reference(&beat(), 20);
    for i in 5..20 {
        assert_eq!(rendered[i], whole[i], "frame {i}");
    }
}

#[test]
fn an_items_beats_land_where_the_metronome_clicks_with_a_first_beat_offset() {
    // 120 bpm at 30 fps: a beat every 15 frames, the first one 4 frames in, as the metronome
    // and the ruler count them.
    let tempo = rastersong_engine::Tempo {
        offset_secs: 4.0 / FPS,
        ..Default::default()
    };
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, beat())]);
    let item = from(span(7, 40, 1), 3.0 / FPS);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![item])]);
    let mut r = Renderer::new(
        &other_backend(),
        &comp,
        &audio(),
        None,
        tempo,
        &Bus::main(),
        &Registry::default(),
        OutputSize::Native,
    )
    .unwrap();
    let rendered = all_frames(&mut r, 40);
    // The phase is 0 at the top of each beat's frame and near 1 just before.
    for beat in [19, 34] {
        assert_eq!(rendered[beat][0], 0, "frame {beat}");
        assert!(rendered[beat - 1][0] > 200, "frame {}", beat - 1);
    }
}

#[test]
fn graph_progress_goes_across_the_item_wherever_it_is() {
    let progress = graph(
        r#"{ "id": "p", "type": "graph_progress" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "p", "to": "o" }"#,
    );
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, progress)]);
    let item = from(span(5, 25, 1), 2.0 / FPS);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![item])]);
    let rendered = all_frames(&mut renderer_for(&comp), 25);
    // The first pixel of each frame: 0 on the item's first frame, half way at its middle.
    assert_eq!(rendered[5][0], 0);
    assert!(
        (127..=128).contains(&rendered[15][0]),
        "{}",
        rendered[15][0]
    );
    assert!(rendered[24][0] > 240, "{}", rendered[24][0]);
}

#[test]
fn an_item_changes_only_the_frames_it_plays() {
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, invert())]);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![span(2, 5, 1)])]);
    let rendered = all_frames(&mut renderer_for(&comp), 8);
    let (plain, inverted) = (reference(&passthrough(), 8), reference(&invert(), 8));
    assert_ne!(plain[3], inverted[3], "the effect shows");
    for i in 0..8 {
        let expected = if (2..5).contains(&i) {
            &inverted
        } else {
            &plain
        };
        assert_eq!(rendered[i], expected[i], "frame {i}");
    }
}

#[test]
fn the_top_graph_wins_and_reads_the_one_below_unless_told_otherwise() {
    // Inverted on the bottom graph track throughout, crushed above it for frames 1..7: by
    // default the crush reads what is below, the inverted picture.
    let both = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "i", "type": "invert" }, { "id": "c", "type": "bitcrush", "params": { "bits": 2 } }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "i" }, { "from": "i", "to": "c" }, { "from": "c", "to": "o" }"#,
    );
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, invert()), (2, crush())]);
    set_graph_tracks(
        &mut comp,
        vec![
            graph_track("Top", vec![span(1, 7, 2)]),
            graph_track("Bottom", vec![span(0, 10, 1)]),
        ],
    );
    let (inverted, crushed, both) = (
        reference(&invert(), 10),
        reference(&crush(), 10),
        reference(&both, 10),
    );
    assert_ne!(both[4], crushed[4]);
    assert_ne!(both[4], inverted[4]);
    let rendered = all_frames(&mut renderer_for(&comp), 10);
    for i in 0..10 {
        let expected = if (1..7).contains(&i) {
            &both
        } else {
            &inverted
        };
        assert_eq!(rendered[i], expected[i], "frame {i}");
    }

    // Reading the mix instead, the top graph replaces the one below where it plays.
    let crush_of = |source: &str| {
        graph(
            &format!(
                r#"{}, {{ "id": "c", "type": "bitcrush", "params": {{ "bits": 2 }} }}, {{ "id": "o", "type": "output" }}"#,
                source_node("s", source, "")
            ),
            r#"{ "from": "s", "to": "c" }, { "from": "c", "to": "o" }"#,
        )
    };
    let mut reading = |source: &str| {
        *graph_of(&mut comp, 2) = GraphDesc::from_json(&crush_of(source)).unwrap();
        all_frames(&mut renderer_for(&comp), 10)
    };
    let rendered = reading("mix");
    assert_eq!(rendered[4], crushed[4]);
    assert_eq!(rendered[8], inverted[8]);

    // Or the bottom graph track's output by name, which is what is below it here.
    let rendered = reading("graph:Bottom");
    assert_eq!(rendered[4], both[4]);
}

#[test]
fn graph_sources_only_point_down() {
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, invert()), (2, show("graph:Top", ""))]);
    set_graph_tracks(
        &mut comp,
        vec![
            graph_track("Top", vec![span(0, 10, 1)]),
            graph_track("Bottom", vec![span(0, 10, 2)]),
        ],
    );
    // The bottom item can't read the track above it: zeros, and the top graph inverts them.
    let rendered = all_frames(&mut renderer_for(&comp), 4);
    assert!(rendered.iter().flatten().all(|&b| b == 255));
}

#[test]
fn a_named_output_is_read_by_graphs_above_without_changing_its_tracks_picture() {
    // The bottom graph passes the picture up as it is and offers it inverted as `Inverted`.
    let offers = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "i", "type": "invert" }, { "id": "o", "type": "output" },
           { "id": "n", "type": "named_output", "params": { "name": "Inverted" } }"#,
        r#"{ "from": "v", "to": "o" }, { "from": "v", "to": "i" }, { "from": "i", "to": "n" }"#,
    );
    let tracks = video_track();
    let mut comp = composition(
        &tracks,
        &[
            (1, offers),
            (2, show("graph:Bottom", "Inverted")),
            (3, show("graph:Bottom", "Nope")),
        ],
    );
    set_graph_tracks(
        &mut comp,
        vec![
            graph_track("Top", vec![span(0, 10, 2)]),
            graph_track("Bottom", vec![span(0, 6, 1)]),
        ],
    );
    // The host renders each graph's render form, which keeps the Named Output.
    for id in 1..=3 {
        let graph = graph_of(&mut comp, id);
        *graph = rastersong_engine::render_form(graph, Registry::shared());
    }
    let (plain, inverted) = (reference(&passthrough(), 10), reference(&invert(), 10));
    let rendered = all_frames(&mut renderer_for(&comp), 10);
    for i in 0..6 {
        assert_eq!(rendered[i], inverted[i], "frame {i}");
    }
    // Where the bottom graph doesn't play, nothing offers it: zeros.
    assert!(rendered[8].iter().all(|&b| b == 0));
    // The bottom track's own picture is the plain one.
    comp.tracks.remove(0);
    let rendered = all_frames(&mut renderer_for(&comp), 6);
    assert_eq!(rendered[3], plain[3]);
    // A name no graph on the track offers reads zeros.
    comp.tracks
        .insert(0, graph_track("Top", vec![span(0, 10, 3)]));
    let rendered = all_frames(&mut renderer_for(&comp), 6);
    assert!(rendered[3].iter().all(|&b| b == 0));
}

#[test]
fn seeking_into_an_item_renders_what_playing_through_does() {
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, delay(2)), (2, delay(1))]);
    set_graph_tracks(
        &mut comp,
        vec![
            graph_track("Top", vec![span(0, FRAMES, 2)]),
            graph_track("Bottom", vec![span(5, 40, 1)]),
        ],
    );
    let expected = all_frames(&mut renderer_for(&comp), FRAMES);
    let mut r = renderer_for(&comp);
    assert!(r.warmup_frames() >= 3, "both delays need history");
    let mut order: Vec<usize> = (0..FRAMES).rev().collect();
    order.extend((0..FRAMES).map(|i| (i * 37 + 11) % FRAMES));
    for i in order {
        let frame = r.render(i, &|| false).unwrap().unwrap();
        assert_eq!(frame, expected[i], "frame {i}");
    }
    // Inside the bottom item, the top delay saw the bottom's: 3 frames behind.
    assert_eq!(expected[30][0], 27);
}

#[test]
fn ports_read_a_tracks_stem_even_when_it_is_out_of_the_mix() {
    // `other` is muted: it is only there to be read.
    let tracks = with_video(vec![late("other")]);
    let mut comp = composition(&tracks, &[(1, show("track:other", ""))]);
    track(&mut comp, "other").in_mix = false;
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![span(0, 10, 1)])]);
    let rendered = all_frames(&mut renderer_for(&comp), 8);
    for (i, frame) in rendered.iter().enumerate().skip(2) {
        assert_eq!(frame[0], (i - 2) as u8, "frame {i} shows `other`");
    }

    // Without a source the node reads zeros.
    *graph_of(&mut comp, 1) = GraphDesc::from_json(&show("", "")).unwrap();
    let rendered = all_frames(&mut renderer_for(&comp), 8);
    assert!(rendered.iter().flatten().all(|&b| b == 0));

    // And the muted track doesn't show in the mix.
    set_graph_tracks(&mut comp, Vec::new());
    let rendered = all_frames(&mut renderer_for(&comp), 8);
    assert_eq!(rendered, reference(&passthrough(), 8));
}

#[test]
fn a_source_with_latency_holds_the_rest_back_to_line_up() {
    // What is below comes a frame late through the bottom graph; the top graph reads it beside
    // the `other` stem, which is held back a frame to meet it.
    let tracks = with_video(vec![late("other")]);
    let side_by_side = graph(
        &format!(
            r#"{{ "id": "v", "type": "video_input" }}, {}, {{ "id": "o", "type": "output" }}"#,
            source_node("x", "track:other", "")
        ),
        r#"{ "from": "v", "to": "o" }"#,
    );
    let mut comp = composition(
        &tracks,
        &[
            (1, shifted()),
            (2, side_by_side),
            (3, show("track:other", "")),
        ],
    );
    track(&mut comp, "other").in_mix = false;
    for (graph, expect) in [(2, 0), (3, 2)] {
        set_graph_tracks(
            &mut comp,
            vec![
                graph_track("Top", vec![span(0, 10, graph)]),
                graph_track("Bottom", vec![span(0, 10, 1)]),
            ],
        );
        let mut r = renderer_for(&comp);
        assert_eq!(r.latency_frames(), 1, "graph {graph}");
        let rendered = all_frames(&mut r, 10);
        for (i, frame) in rendered.iter().enumerate().skip(expect) {
            assert_eq!(frame[0], (i - expect) as u8, "graph {graph}, frame {i}");
        }
    }
}

#[test]
fn the_mix_shows_the_top_video_track_playing() {
    // `top` plays half a second of the clip from 1 s, over `bottom` playing all of it.
    let top = ComposedTrack::new(
        "top",
        TrackKind::Video,
        vec![Item {
            end: Some(0.5),
            ..Item::whole(CLIP, 1.0)
        }],
    );
    let bottom = ComposedTrack::new("bottom", TrackKind::Video, vec![Item::whole(CLIP, 0.0)]);
    let tracks = vec![top, bottom];
    let comp = composition(&tracks, &[]);
    let plain = reference(&passthrough(), FRAMES);
    let rendered = all_frames(&mut renderer_for(&comp), FRAMES);
    for (i, frame) in rendered.iter().enumerate() {
        let shown = if (30..45).contains(&i) { i - 30 } else { i };
        assert_eq!(*frame, plain[shown], "frame {i}");
    }
}

#[test]
fn pre_roll_warms_an_item_up_before_its_first_frame() {
    let first_pixel = |pre_roll: bool| {
        let tracks = video_track();
        let mut comp = composition(&tracks, &[(1, delay(2))]);
        let mut item = span(6, 10, 1);
        item.pre_roll = pre_roll;
        set_graph_tracks(&mut comp, vec![graph_track("G", vec![item])]);
        let mut r = renderer_for(&comp);
        // Rendered from the start, so the only difference is the item's own start.
        all_frames(&mut r, 10)[6][0]
    };
    // The delayed picture is 2 frames old (frame 4) when the graph ran before the item, and
    // still empty when it started cold at the item's edge.
    assert_eq!(first_pixel(true), 4);
    assert_eq!(first_pixel(false), 0);
}

#[test]
fn items_with_different_latencies_stay_frame_aligned() {
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, passthrough()), (2, shifted())]);
    set_graph_tracks(
        &mut comp,
        vec![graph_track("G", vec![span(0, 4, 1), span(4, 8, 2)])],
    );
    let mut r = renderer_for(&comp);
    assert_eq!(
        r.latency_frames(),
        1,
        "the graph track takes its slowest item's latency"
    );
    let rendered = all_frames(&mut r, 10);
    let (plain, alone) = (reference(&passthrough(), 10), reference(&shifted(), 10));
    for i in 0..4 {
        assert_eq!(
            rendered[i], plain[i],
            "the quicker item is held back: frame {i}"
        );
    }
    for i in 4..8 {
        assert_eq!(rendered[i], alone[i], "frame {i}");
    }
    assert_eq!(
        rendered[9], plain[9],
        "after the items, the mix plays on in step"
    );
}

#[test]
fn taps_read_the_graph_that_ran() {
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, invert())]);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![span(2, 5, 1)])]);
    let mut r = renderer_for(&comp);
    r.render(3, &|| false).unwrap();
    assert!(r.tap("i", 0).is_some());
    assert!(!r.levels().is_empty());
    assert!(!r.node_stats().is_empty());
    r.render(6, &|| false).unwrap();
    assert!(r.tap("i", 0).is_none(), "no graph ran at frame 6");
    assert!(r.levels().is_empty());
}

#[test]
fn a_graph_that_cannot_compile_fails_the_build_naming_it() {
    let broken = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "nowhere" }"#,
    );
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(7, broken)]);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![span(0, 4, 7)])]);
    let error = build(&comp).unwrap_err();
    assert!(
        matches!(
            error,
            EngineError::Fx {
                graph: ResourceId(7),
                ..
            }
        ),
        "{error:?}"
    );
    // Named by its resource's name, not its id.
    assert!(error.to_string().contains("graph 7"), "{error}");
}

fn song() -> ComposedTrack {
    ComposedTrack::new("song", TrackKind::Audio, vec![Item::whole(SONG, 0.0)])
}

/// The loudest sample the first 8 frames of `comp` render.
fn peak(comp: &Composition) -> f32 {
    let mut r = renderer_for(comp);
    assert!(matches!(r.audio_sink(), AudioSink::Rendered { .. }));
    (0..8)
        .flat_map(|i| {
            r.render(i, &|| false).unwrap();
            r.audio().expect("every frame has a block").samples.clone()
        })
        .fold(0.0f32, |m, s| m.max(s.abs()))
}

/// The main sound straight to the Audio Output, with a gain so it isn't the plain mix.
fn louder() -> String {
    louder_of(r#"{ "id": "a", "type": "audio_input" }"#)
}

/// What node `a` reads straight to the Audio Output, with a gain so it isn't the plain mix.
fn louder_of(a: &str) -> String {
    graph(
        &format!(
            r#"{a}, {{ "id": "s", "type": "audio_output", "params": {{ "volume": 0.001 }} }}, {{ "id": "v", "type": "video_input" }}, {{ "id": "o", "type": "output" }}"#
        ),
        r#"{ "from": "a", "to": "s" }, { "from": "v", "to": "o" }"#,
    )
}

#[test]
fn a_graph_hears_the_mix_at_the_tracks_volume_or_a_stem_as_it_is() {
    let tracks = with_video(vec![song()]);
    let mut comp = composition(&tracks, &[(1, louder())]);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![span(0, 10, 1)])]);
    let full = peak(&comp);
    track(&mut comp, "song").gain = 0.5;
    let half = peak(&comp);
    assert!(full > 0.1, "{full}");
    assert!((half / full - 0.5).abs() < 0.01, "{half} / {full}");
    track(&mut comp, "song").in_mix = false;
    assert!(peak(&comp) < 1e-6, "a muted track is silent in the mix");
    // Its stem is there as it is, at full level.
    *graph_of(&mut comp, 1) = GraphDesc::from_json(&louder_of(
        r#"{ "id": "a", "type": "audio_input", "params": { "source": "track:song" } }"#,
    ))
    .unwrap();
    assert!((peak(&comp) / full - 1.0).abs() < 0.01);
}

#[test]
fn a_graph_passing_the_mix_through_leaves_the_sound_to_the_mixer_and_one_without_sound_is_silent() {
    let template = graph(
        r#"{ "id": "a", "type": "audio_input" }, { "id": "s", "type": "audio_output" }, { "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "a", "to": "s" }, { "from": "v", "to": "o" }"#,
    );
    let tracks = with_video(vec![song()]);
    let mut comp = composition(&tracks, &[(1, template), (2, passthrough())]);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![span(0, 10, 1)])]);
    // The template's sound is the mix, so the real-time mixer plays it.
    assert!(matches!(
        renderer_for(&comp).audio_sink(),
        AudioSink::TrackMix
    ));
    // Stacked on itself, still the mix.
    push_graph_track(&mut comp, graph_track("Bottom", vec![span(0, 10, 1)]));
    assert!(matches!(
        renderer_for(&comp).audio_sink(),
        AudioSink::TrackMix
    ));
    // A graph with no Audio Output is silent where it plays: nothing passes through.
    comp.tracks[0].items[0].resource = ResourceId(2);
    assert!(peak(&comp) < 1e-6);
}

/// Frames recorded by the offline render.
#[derive(Default)]
struct Recorder(Vec<Vec<u8>>);

impl FrameSink for Recorder {
    fn start(&mut self, _: &RenderInfo, _: &AudioSink) -> Result<(), EngineError> {
        Ok(())
    }

    fn frame(&mut self, frame: RenderedFrame) -> Result<(), EngineError> {
        self.0.push(frame.rgb.to_vec());
        Ok(())
    }
}

#[test]
fn offline_renders_the_composition() {
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, invert())]);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![span(0, FRAMES, 1)])]);
    let settings = RenderSettings {
        range: Some(0..8),
        ..Default::default()
    };
    let mut sink = Recorder::default();
    render(
        &other_backend(),
        &comp,
        &audio(),
        &settings,
        &mut sink,
        &mut |_, _| true,
    )
    .unwrap();
    assert_eq!(sink.0, reference(&invert(), 8));
}

fn engine() -> Engine {
    Engine::new(Arc::new(other_backend()), EngineConfig::default())
}

fn engine_frames(engine: &Engine, count: usize) -> Vec<Vec<u8>> {
    wait_until("the frames", || engine.buffered_from(0) >= count);
    (0..count)
        .map(|i| engine.frame(i).unwrap().rgb.clone())
        .collect()
}

#[test]
fn the_service_renders_the_composition_and_it_is_part_of_the_cache_key() {
    let engine = engine();
    engine.set_composition(composition(&video_track(), &[]));
    let (plain, inverted) = (reference(&passthrough(), 8), reference(&invert(), 8));
    assert_eq!(engine_frames(&engine, 8), plain);

    let mut comp = composition(&video_track(), &[(1, invert())]);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![span(0, FRAMES, 1)])]);
    engine.set_composition(comp.clone());
    wait_until("the composition renders", || {
        engine.frame(3).is_some_and(|f| f.rgb == inverted[3])
    });
    assert_eq!(engine_frames(&engine, 8), inverted);
    assert_eq!(engine.status(), EngineStatus::Ready);

    // The same composition again, and edits to what the graphs don't render, keep the cache.
    let kept = engine.frame(3).unwrap();
    engine.set_composition(comp.clone());
    let mut labelled = comp.clone();
    graph_of(&mut labelled, 1).nodes[0].label = Some("Picture".into());
    engine.set_composition(labelled);
    assert!(Arc::ptr_eq(&kept, &engine.frame(3).unwrap()));

    // Bypassing every graph shows the mix.
    engine.set_bypass_all(true);
    wait_until("the bypassed render", || {
        engine.frame(3).is_some_and(|f| f.rgb == plain[3])
    });

    // An item from frame 2.
    engine.set_bypass_all(false);
    comp.tracks[0].items = vec![span(2, FRAMES, 1)];
    engine.set_composition(comp);
    wait_until("the later item", || {
        engine.frame(3).is_some_and(|f| f.rgb == inverted[3])
    });
    assert_eq!(engine.frame(1).unwrap().rgb, plain[1]);

    // Without graph items the mix plays.
    engine.set_composition(composition(&video_track(), &[]));
    wait_until("the mix", || {
        engine.frame(3).is_some_and(|f| f.rgb == plain[3])
    });
}

#[test]
fn inspecting_another_graph_keeps_every_rendered_frame() {
    let engine = engine();
    let two = |first: String| {
        let mut comp = composition(&video_track(), &[(1, first), (2, crush())]);
        set_graph_tracks(
            &mut comp,
            vec![graph_track("G", vec![span(0, 4, 1), span(4, FRAMES, 2)])],
        );
        comp
    };
    let has_node = |id: &str| engine.node_stats().iter().any(|s| &*s.node == id);
    engine.set_composition(two(invert()));
    engine.set_inspected(Some(ResourceId(1)));
    let rendered = engine_frames(&engine, 8);
    wait_until("graph 1's stats", || has_node("i"));
    let kept = engine.frame(3).unwrap();
    assert_eq!(kept.inspected, Some(ResourceId(1)));

    // Inspecting graph 2 renders the same frames...
    engine.set_inspected(Some(ResourceId(2)));
    // ...so it rebuilds only for the editor, which now inspects graph 2's item...
    wait_until("graph 2's stats", || has_node("c"));
    // ...with the frames all kept.
    assert!(Arc::ptr_eq(&kept, &engine.frame(3).unwrap()));
    assert_eq!(engine_frames(&engine, 8), rendered);

    // Editing a graph is still an edit.
    engine.set_composition(two(passthrough()));
    wait_until("the edit", || {
        engine.frame(3).is_none_or(|f| !Arc::ptr_eq(&kept, &f))
    });
}

#[test]
fn the_service_reports_a_graph_that_cannot_compile() {
    let engine = engine();
    let broken = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "b", "type": "bogus" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "b" }, { "from": "b", "to": "o" }"#,
    );
    let mut comp = composition(&video_track(), &[(7, broken)]);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![span(0, FRAMES, 7)])]);
    engine.set_composition(comp);
    wait_until("the failure", || {
        matches!(engine.status(), EngineStatus::Failed(_))
    });
    let EngineStatus::Failed(failure) = engine.status() else {
        unreachable!()
    };
    assert!(failure.message.contains("graph 7"), "{}", failure.message);
    // The node at fault is named with the graph it belongs to.
    assert_eq!(failure.graph, Some(ResourceId(7)));
    assert_eq!(failure.node.as_deref(), Some("b"));
}

#[test]
fn a_graph_only_project_renders_its_items_length() {
    // No video or audio at all: a generator on a graph track, for 10 frames.
    let mut comp = composition(&[], &[(1, beat())]);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![span(0, 10, 1)])]);
    let mut r = renderer_for(&comp);
    // With no video to take a timebase from, the default one.
    assert_eq!((r.info().frames, r.info().width), (10, 1920));
    assert_eq!(all_frames(&mut r, 10)[9][0], reference(&beat(), 10)[9][0]);

    // The service renders it too, rather than waiting for a file.
    let engine = engine();
    engine.set_composition(comp);
    wait_until("the frames", || engine.buffered_from(0) >= 10);
    assert_eq!(engine.status(), EngineStatus::Ready);
    assert_eq!(engine.info().unwrap().frames, 10);
}

#[test]
fn a_track_plays_items_of_different_files() {
    // Half a second of the clip, then the short clip to its end: each item reads its own file,
    // so the second lasts the short clip's 10 frames.
    let tracks = vec![ComposedTrack::new(
        "Video",
        TrackKind::Video,
        vec![
            Item {
                end: Some(0.5),
                ..Item::whole(CLIP, 0.0)
            },
            Item::whole(SHORT, 0.5),
        ],
    )];
    let comp = composition(&tracks, &[]);
    let mut r = renderer_for(&comp);
    assert_eq!(r.info().frames, 25);
    let rendered = all_frames(&mut r, 25);
    for (i, frame) in rendered.iter().enumerate() {
        let shown = if i < 15 { i } else { i - 15 };
        assert_eq!(frame[0], shown as u8, "frame {i}");
    }
}

/// An audio track `mixed` playing the song for 4 frames, then the quiet sound.
fn mixed() -> ComposedTrack {
    ComposedTrack::new(
        "mixed",
        TrackKind::Audio,
        vec![
            Item {
                end: Some(4.0 / FPS),
                ..Item::whole(SONG, 0.0)
            },
            Item::whole(QUIET, 4.0 / FPS),
        ],
    )
}

#[test]
fn a_stem_reads_the_item_playing() {
    // The stem straight to the Audio Output: rendered sound, not the plain mix.
    let stem =
        louder_of(r#"{ "id": "a", "type": "audio_input", "params": { "source": "track:mixed" } }"#);
    let tracks = with_video(vec![mixed()]);
    let mut comp = composition(&tracks, &[(1, stem)]);
    set_graph_tracks(&mut comp, vec![graph_track("G", vec![span(0, 10, 1)])]);
    let mut r = renderer_for(&comp);
    let peaks: Vec<f32> = (0..9)
        .map(|i| {
            r.render(i, &|| false).unwrap();
            let block = r.audio().expect("every frame has a block");
            block.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
        })
        .collect();
    // The song is loud; the quiet sound, after its fade in, is 0.1.
    assert!(peaks[2] > 0.5, "{peaks:?}");
    for (i, peak) in peaks.iter().enumerate().skip(6) {
        assert!((peak - 0.1).abs() < 0.01, "frame {i}: {peaks:?}");
    }
}

#[test]
fn a_stem_takes_the_widest_layout_of_its_files() {
    // The song is mono at 9 kHz, the quiet sound stereo at 48 kHz: the stem is stereo, in the
    // quiet sound's longer blocks.
    let tracks = with_video(vec![mixed()]);
    let comp = composition(&tracks, &[]);
    let r = renderer_for(&comp);
    assert_eq!(
        r.compile_options().sources.get("mixed"),
        Some(&rastersong_engine::Layout::audio_channels(1600, 2))
    );
}

#[test]
fn video_and_graph_tracks_stack_the_same_way() {
    // The other clip over the clip for frames 30..45: once on a video track above, once on a
    // graph track showing a muted video track's stem. Elsewhere what is below shows through.
    let over = |position: f64| {
        ComposedTrack::new(
            "other",
            TrackKind::Video,
            vec![Item {
                end: Some(0.5),
                ..Item::whole(OTHER, position)
            }],
        )
    };
    let bottom = ComposedTrack::new("bottom", TrackKind::Video, vec![Item::whole(CLIP, 0.0)]);
    let video = composition(&[over(1.0), bottom.clone()], &[]);
    let mut muted = over(1.0);
    muted.in_mix = false;
    let mut graphs = composition(&[muted, bottom], &[(1, show("track:other", ""))]);
    set_graph_tracks(&mut graphs, vec![graph_track("G", vec![span(30, 45, 1)])]);
    let (a, b) = (
        all_frames(&mut renderer_for(&video), FRAMES),
        all_frames(&mut renderer_for(&graphs), FRAMES),
    );
    for i in 0..FRAMES {
        assert_eq!(a[i], b[i], "frame {i}");
        let shown = if (30..45).contains(&i) { i - 30 } else { i };
        assert_eq!(a[i][0], shown as u8, "frame {i}");
    }
}

#[test]
fn a_muted_graph_track_passes_what_is_below() {
    // The top graph shows the middle graph track, which is muted: it reads what is below the
    // middle track, the plain video, not zeros.
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, show("graph:Middle", "")), (2, invert())]);
    set_graph_tracks(
        &mut comp,
        vec![
            graph_track("Top", vec![span(0, 10, 1)]),
            graph_track("Middle", vec![span(0, 10, 2)]),
        ],
    );
    track(&mut comp, "Middle").in_mix = false;
    let plain = reference(&passthrough(), 10);
    assert_eq!(all_frames(&mut renderer_for(&comp), 10), plain);
    // Unmuted, it shows the inverted picture.
    track(&mut comp, "Middle").in_mix = true;
    assert_eq!(
        all_frames(&mut renderer_for(&comp), 10),
        reference(&invert(), 10)
    );
    // `graph:` and `track:` name a track the same way, whatever its kind.
    *graph_of(&mut comp, 1) = GraphDesc::from_json(&show("graph:Video", "")).unwrap();
    assert_eq!(all_frames(&mut renderer_for(&comp), 10), plain);
}
