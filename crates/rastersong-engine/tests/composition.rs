//! Composition: the tracks' mix and the graph tracks above it, each item filling its ports from
//! its sources, rendered by the sequential renderer, the service and the offline render.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{FRAMES, backend, tracks, wait_until};
use rastersong_engine::{
    AudioSink, Bus, ComposedGraphTrack, Composition, Engine, EngineConfig, EngineError,
    EngineStatus, FakeVideo, FrameSink, GraphDesc, GraphItem, Item, MediaTrack, OutputSize,
    Rational, Registry, RenderInfo, RenderSettings, RenderTrack, RenderedFrame, Renderer, Source,
    Timeline, TrackKind, TrackSpec, render,
};

const FPS: f64 = 30.0;

/// The open graph, which none of these tests uses unless they say so.
const OPEN: u32 = 0;

fn graph(nodes: &str, connections: &str) -> String {
    format!(r#"{{ "version": 0, "nodes": [ {nodes} ], "connections": [ {connections} ] }}"#)
}

fn passthrough() -> String {
    graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "o" }"#,
    )
}

/// Port `port`'s picture straight to the output.
fn show(port: &str) -> String {
    graph(
        &format!(
            r#"{{ "id": "v", "type": "video_input", "params": {{ "port": "{port}" }} }}, {{ "id": "o", "type": "output" }}"#
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
            r#"{{ "id": "v", "type": "video_input" }}, {{ "id": "d", "type": "delay", "params": {{ "time": {frames}, "unit": "frame", "mix": 1 }} }}, {{ "id": "o", "type": "output" }}"#
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
fn span(from: usize, to: usize, graph: u32) -> GraphItem {
    GraphItem::new(graph, from as f64 / FPS, (to - from) as f64 / FPS)
}

/// A graph track called `name` holding `items`.
fn graph_track(name: &str, items: Vec<GraphItem>) -> ComposedGraphTrack {
    ComposedGraphTrack {
        name: name.to_owned(),
        items,
    }
}

/// Every track of `tracks` in the mix, no graph tracks, and `graphs` to place.
fn composition(tracks: &[RenderTrack], graphs: &[(u32, String)]) -> Composition {
    Composition {
        tracks: tracks
            .iter()
            .map(|t| {
                let kind = match t.media {
                    rastersong_engine::TrackMedia::Video { .. } => TrackKind::Video,
                    rastersong_engine::TrackMedia::Audio(_) => TrackKind::Audio,
                };
                MediaTrack::new(t.name.clone(), kind)
            })
            .collect(),
        graph_tracks: Vec::new(),
        graphs: graphs
            .iter()
            .map(|(id, json)| (*id, GraphDesc::from_json(json).unwrap()))
            .collect(),
        open: OPEN,
    }
}

fn build(composition: &Composition, tracks: &[RenderTrack]) -> Result<Renderer, EngineError> {
    Renderer::with_composition(
        &other_backend(),
        None,
        tracks,
        composition,
        &GraphDesc::from_json(&passthrough()).unwrap(),
        Default::default(),
        &Bus::main(),
        &Registry::default(),
        OutputSize::Native,
    )
}

fn renderer_for(composition: &Composition, tracks: &[RenderTrack]) -> Renderer {
    build(composition, tracks).unwrap()
}

fn other_backend() -> rastersong_engine::FakeBackend {
    backend().with_video(
        "other",
        FakeVideo {
            width: 16,
            height: 8,
            frame_count: FRAMES,
            frame_rate: Rational::new(30, 1),
        },
    )
}

fn video_track() -> Vec<RenderTrack> {
    tracks(Vec::new())
}

/// A video track `name` playing the `other` clip 2 frames late.
fn late(name: &str) -> RenderTrack {
    let mut other = RenderTrack::video(name, "other");
    other.items = vec![Item::whole(2.0 / FPS)];
    other
}

fn all_frames(r: &mut Renderer, frames: usize) -> Vec<Vec<u8>> {
    (0..frames)
        .map(|i| r.render(i, &|| false).unwrap().unwrap().to_vec())
        .collect()
}

/// A graph rendered over the video track for the whole timeline.
fn reference(json: &str, frames: usize) -> Vec<Vec<u8>> {
    let mut r = Renderer::new(
        &other_backend(),
        None,
        &video_track(),
        &GraphDesc::from_json(json).unwrap(),
        Default::default(),
        &Bus::main(),
        &Registry::default(),
        OutputSize::Native,
    )
    .unwrap();
    all_frames(&mut r, frames)
}

#[test]
fn without_graph_items_the_mix_shows() {
    let tracks = video_track();
    let mut r = renderer_for(&composition(&tracks, &[]), &tracks);
    assert_eq!(r.latency_frames(), 0);
    assert!(matches!(r.audio_sink(), AudioSink::TrackMix));
    assert_eq!(all_frames(&mut r, 8), reference(&passthrough(), 8));
}

#[test]
fn an_item_changes_only_the_frames_it_plays() {
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, invert())]);
    comp.graph_tracks = vec![graph_track("G", vec![span(2, 5, 1)])];
    let rendered = all_frames(&mut renderer_for(&comp, &tracks), 8);
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
    comp.graph_tracks = vec![
        graph_track("Top", vec![span(1, 7, 2)]),
        graph_track("Bottom", vec![span(0, 10, 1)]),
    ];
    let (inverted, crushed, both) = (
        reference(&invert(), 10),
        reference(&crush(), 10),
        reference(&both, 10),
    );
    assert_ne!(both[4], crushed[4]);
    assert_ne!(both[4], inverted[4]);
    let rendered = all_frames(&mut renderer_for(&comp, &tracks), 10);
    for i in 0..10 {
        let expected = if (1..7).contains(&i) {
            &both
        } else {
            &inverted
        };
        assert_eq!(rendered[i], expected[i], "frame {i}");
    }

    // Reading the mix instead, the top graph replaces the one below where it plays.
    comp.graph_tracks[0].items[0]
        .sources
        .insert("Video".into(), Source::Mix);
    let rendered = all_frames(&mut renderer_for(&comp, &tracks), 10);
    assert_eq!(rendered[4], crushed[4]);
    assert_eq!(rendered[8], inverted[8]);

    // Or the bottom graph track's output by name, which is what is below it here.
    comp.graph_tracks[0].items[0].sources.insert(
        "Video".into(),
        Source::Graph {
            track: "Bottom".into(),
            port: "Video".into(),
        },
    );
    let rendered = all_frames(&mut renderer_for(&comp, &tracks), 10);
    assert_eq!(rendered[4], both[4]);
}

#[test]
fn graph_sources_only_point_down() {
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, invert()), (2, show("Up"))]);
    let mut reader = span(0, 10, 2);
    reader.sources.insert(
        "Up".into(),
        Source::Graph {
            track: "Top".into(),
            port: "Video".into(),
        },
    );
    comp.graph_tracks = vec![
        graph_track("Top", vec![span(0, 10, 1)]),
        graph_track("Bottom", vec![reader]),
    ];
    // The bottom item can't read the track above it: zeros, and the top graph inverts them.
    let rendered = all_frames(&mut renderer_for(&comp, &tracks), 4);
    assert!(rendered.iter().flatten().all(|&b| b == 255));
}

#[test]
fn seeking_into_an_item_renders_what_playing_through_does() {
    let tracks = video_track();
    let mut comp = composition(&tracks, &[(1, delay(2)), (2, delay(1))]);
    comp.graph_tracks = vec![
        graph_track("Top", vec![span(0, FRAMES, 2)]),
        graph_track("Bottom", vec![span(5, 40, 1)]),
    ];
    let expected = all_frames(&mut renderer_for(&comp, &tracks), FRAMES);
    let mut r = renderer_for(&comp, &tracks);
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
    let tracks = tracks(vec![late("other")]);
    let mut comp = composition(&tracks, &[(1, show("Other"))]);
    comp.tracks[1].in_mix = false;
    let mut item = span(0, 10, 1);
    item.sources
        .insert("Other".into(), Source::Track("other".into()));
    comp.graph_tracks = vec![graph_track("G", vec![item])];
    let rendered = all_frames(&mut renderer_for(&comp, &tracks), 8);
    for (i, frame) in rendered.iter().enumerate().skip(2) {
        assert_eq!(frame[0], (i - 2) as u8, "frame {i} shows `other`");
    }

    // Without a source the port reads zeros.
    comp.graph_tracks[0].items[0].sources.clear();
    let rendered = all_frames(&mut renderer_for(&comp, &tracks), 8);
    assert!(rendered.iter().flatten().all(|&b| b == 0));

    // And the muted track doesn't show in the mix.
    comp.graph_tracks.clear();
    let rendered = all_frames(&mut renderer_for(&comp, &tracks), 8);
    assert_eq!(rendered, reference(&passthrough(), 8));
}

#[test]
fn a_source_with_latency_holds_the_rest_back_to_line_up() {
    // What is below comes a frame late through the bottom graph; the top graph reads it beside
    // the `other` stem, which is held back a frame to meet it.
    let tracks = tracks(vec![late("other")]);
    let side_by_side = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "x", "type": "video_input", "params": { "port": "Other" } }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "o" }"#,
    );
    let mut comp = composition(
        &tracks,
        &[(1, shifted()), (2, side_by_side), (3, show("Other"))],
    );
    comp.tracks[1].in_mix = false;
    for (graph, expect) in [(2, 0), (3, 2)] {
        let mut top = span(0, 10, graph);
        top.sources
            .insert("Other".into(), Source::Track("other".into()));
        comp.graph_tracks = vec![
            graph_track("Top", vec![top]),
            graph_track("Bottom", vec![span(0, 10, 1)]),
        ];
        let mut r = renderer_for(&comp, &tracks);
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
    let top = RenderTrack {
        items: vec![Item {
            end: Some(0.5),
            ..Item::whole(1.0)
        }],
        ..RenderTrack::video("top", common::VIDEO)
    };
    let tracks = vec![top, RenderTrack::video("bottom", common::VIDEO)];
    let comp = composition(&tracks, &[]);
    let plain = reference(&passthrough(), FRAMES);
    let rendered = all_frames(&mut renderer_for(&comp, &tracks), FRAMES);
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
        comp.graph_tracks = vec![graph_track("G", vec![item])];
        let mut r = renderer_for(&comp, &tracks);
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
    comp.graph_tracks = vec![graph_track("G", vec![span(0, 4, 1), span(4, 8, 2)])];
    let mut r = renderer_for(&comp, &tracks);
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
    comp.graph_tracks = vec![graph_track("G", vec![span(2, 5, 1)])];
    let mut r = renderer_for(&comp, &tracks);
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
    comp.graph_tracks = vec![graph_track("G", vec![span(0, 4, 7)])];
    let error = build(&comp, &tracks).unwrap_err();
    assert!(
        matches!(error, EngineError::Fx { graph: 7, .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains('7'), "{error}");
}

fn song() -> RenderTrack {
    RenderTrack::audio(
        "song",
        Arc::new(rastersong_engine::sources::Modulator::new(&common::audio())),
        0.0,
    )
}

/// The loudest sample the first 8 frames of `comp` render.
fn peak(comp: &Composition, tracks: &[RenderTrack]) -> f32 {
    let mut r = renderer_for(comp, tracks);
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
    graph(
        r#"{ "id": "a", "type": "audio_input" }, { "id": "s", "type": "audio_output", "params": { "volume": 0.001 } }, { "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "a", "to": "s" }, { "from": "v", "to": "o" }"#,
    )
}

#[test]
fn a_graph_hears_the_mix_at_the_tracks_volume_or_a_stem_as_it_is() {
    let tracks = tracks(vec![song()]);
    let mut comp = composition(&tracks, &[(1, louder())]);
    comp.graph_tracks = vec![graph_track("G", vec![span(0, 10, 1)])];
    let full = peak(&comp, &tracks);
    comp.tracks[1].volume = 0.5;
    let half = peak(&comp, &tracks);
    assert!(full > 0.1, "{full}");
    assert!((half / full - 0.5).abs() < 0.01, "{half} / {full}");
    comp.tracks[1].in_mix = false;
    assert!(
        peak(&comp, &tracks) < 1e-6,
        "a muted track is silent in the mix"
    );
    // Its stem is there as it is, at full level.
    comp.graph_tracks[0].items[0]
        .sources
        .insert("Audio".into(), Source::Track("song".into()));
    assert!((peak(&comp, &tracks) / full - 1.0).abs() < 0.01);
}

#[test]
fn a_graph_passing_the_mix_through_leaves_the_sound_to_the_mixer_and_one_without_sound_is_silent() {
    let template = graph(
        r#"{ "id": "a", "type": "audio_input" }, { "id": "s", "type": "audio_output" }, { "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "a", "to": "s" }, { "from": "v", "to": "o" }"#,
    );
    let tracks = tracks(vec![song()]);
    let mut comp = composition(&tracks, &[(1, template), (2, passthrough())]);
    comp.graph_tracks = vec![graph_track("G", vec![span(0, 10, 1)])];
    // The template's sound is the mix, so the real-time mixer plays it.
    assert!(matches!(
        renderer_for(&comp, &tracks).audio_sink(),
        AudioSink::TrackMix
    ));
    // Stacked on itself, still the mix.
    comp.graph_tracks
        .push(graph_track("Bottom", vec![span(0, 10, 1)]));
    assert!(matches!(
        renderer_for(&comp, &tracks).audio_sink(),
        AudioSink::TrackMix
    ));
    // A graph with no Audio Output is silent where it plays: nothing passes through.
    comp.graph_tracks[0].items[0].graph = 2;
    assert!(peak(&comp, &tracks) < 1e-6);
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
    comp.graph_tracks = vec![graph_track("G", vec![span(0, FRAMES, 1)])];
    let settings = RenderSettings {
        range: Some(0..8),
        composition: Some(comp),
        ..Default::default()
    };
    let mut sink = Recorder::default();
    render(
        &other_backend(),
        &tracks,
        &GraphDesc::from_json(&passthrough()).unwrap(),
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

fn video_timeline() -> Timeline {
    Timeline {
        tracks: vec![TrackSpec::new(
            "Video",
            TrackKind::Video,
            PathBuf::from(common::VIDEO),
        )],
        ..Timeline::default()
    }
}

#[test]
fn the_service_renders_the_composition_and_it_is_part_of_the_cache_key() {
    let engine = engine();
    engine.set_timeline(video_timeline());
    engine.set_graph(GraphDesc::from_json(&passthrough()).unwrap());
    let (plain, inverted) = (reference(&passthrough(), 8), reference(&invert(), 8));
    assert_eq!(engine_frames(&engine, 8), plain);

    let mut comp = composition(&video_track(), &[(1, invert())]);
    comp.graph_tracks = vec![graph_track("G", vec![span(0, FRAMES, 1)])];
    engine.set_composition(Some(comp.clone()));
    wait_until("the composition renders", || {
        engine.frame(3).is_some_and(|f| f.rgb == inverted[3])
    });
    assert_eq!(engine_frames(&engine, 8), inverted);
    assert_eq!(engine.status(), EngineStatus::Ready);

    // The same composition again, and edits to what the graphs don't render, keep the cache.
    let kept = engine.frame(3).unwrap();
    engine.set_composition(Some(comp.clone()));
    let mut labelled = comp.clone();
    labelled.graphs[0].1.nodes[0].label = Some("Picture".into());
    engine.set_composition(Some(labelled));
    assert!(Arc::ptr_eq(&kept, &engine.frame(3).unwrap()));

    // Bypassing every graph shows the mix.
    engine.set_bypass_all(true);
    wait_until("the bypassed render", || {
        engine.frame(3).is_some_and(|f| f.rgb == plain[3])
    });

    // An item from frame 2.
    engine.set_bypass_all(false);
    comp.graph_tracks[0].items = vec![span(2, FRAMES, 1)];
    engine.set_composition(Some(comp));
    wait_until("the later item", || {
        engine.frame(3).is_some_and(|f| f.rgb == inverted[3])
    });
    assert_eq!(engine.frame(1).unwrap().rgb, plain[1]);

    // Without a composition the open graph plays over the whole timeline.
    engine.set_composition(None);
    wait_until("the open graph", || {
        engine.frame(3).is_some_and(|f| f.rgb == plain[3])
    });
}

#[test]
fn opening_another_graph_keeps_every_rendered_frame() {
    let engine = engine();
    engine.set_timeline(video_timeline());
    let open_one = |open: u32, other: (u32, String)| {
        let mut comp = composition(&video_track(), &[other]);
        comp.open = open;
        comp.graph_tracks = vec![graph_track("G", vec![span(0, 4, 1), span(4, FRAMES, 2)])];
        comp
    };
    let has_node = |id: &str| engine.node_stats().iter().any(|s| &*s.node == id);
    engine.set_graphs(
        GraphDesc::from_json(&invert()).unwrap(),
        Some(open_one(1, (2, crush()))),
    );
    let rendered = engine_frames(&engine, 8);
    assert!(has_node("i"));
    let kept = engine.frame(3).unwrap();
    assert_eq!(kept.open, Some(1));

    // Graph 2 opened: graph 1 goes back among the stored graphs, which renders the same.
    engine.set_graphs(
        GraphDesc::from_json(&crush()).unwrap(),
        Some(open_one(2, (1, invert()))),
    );
    // Rebuilt for the editor, which now inspects graph 2's item...
    wait_until("the open graph's stats", || has_node("c"));
    // ...with the frames all kept.
    assert!(Arc::ptr_eq(&kept, &engine.frame(3).unwrap()));
    assert_eq!(engine_frames(&engine, 8), rendered);

    // Editing the open graph is still an edit.
    engine.set_graphs(
        GraphDesc::from_json(&passthrough()).unwrap(),
        Some(open_one(2, (1, invert()))),
    );
    wait_until("the edit", || {
        engine.frame(3).is_none_or(|f| !Arc::ptr_eq(&kept, &f))
    });
}

#[test]
fn the_service_reports_a_graph_that_cannot_compile() {
    let engine = engine();
    engine.set_timeline(video_timeline());
    engine.set_graph(GraphDesc::from_json(&passthrough()).unwrap());
    let broken = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "b", "type": "bogus" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "b" }, { "from": "b", "to": "o" }"#,
    );
    let mut comp = composition(&video_track(), &[(7, broken)]);
    comp.graph_tracks = vec![graph_track("G", vec![span(0, FRAMES, 7)])];
    engine.set_composition(Some(comp));
    wait_until("the failure", || {
        matches!(engine.status(), EngineStatus::Failed(_))
    });
    let EngineStatus::Failed(failure) = engine.status() else {
        unreachable!()
    };
    assert!(failure.message.contains('7'), "{}", failure.message);
    // The node at fault is named with the graph it belongs to.
    assert_eq!(failure.graph, Some(7));
    assert_eq!(failure.node.as_deref(), Some("b"));
}
