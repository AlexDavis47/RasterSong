//! Graph layers: items on lanes above the tracks, rendered by the sequential renderer, the
//! service and the offline render.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use common::{FRAMES, VIDEO, backend, tracks, wait_until};
use rastersong_engine::{
    AudioSink, Binding, Bus, Engine, EngineConfig, EngineError, EngineStatus, FakeVideo, FrameSink,
    GraphDesc, Item, LayerSet, OutputSize, Rational, Registry, RenderInfo, RenderItem,
    RenderSettings, RenderTrack, RenderedFrame, Renderer, Timeline, TrackKind, TrackSpec, render,
};

const FPS: f64 = 30.0;

/// The open graph, which none of these tests places unless they say so.
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

/// The picture of the project delayed by `frames` frames.
fn delay(frames: u32) -> String {
    graph(
        &format!(
            r#"{{ "id": "v", "type": "video_input" }}, {{ "id": "d", "type": "delay", "params": {{ "time": {frames}, "unit": "frame", "mix": 1 }} }}, {{ "id": "o", "type": "output" }}"#
        ),
        r#"{ "from": "v", "to": "d" }, { "from": "d", "to": "o" }"#,
    )
}

/// The same graph in a layer of the stack: its input reads the layer below.
fn below() -> BTreeMap<String, Binding> {
    BTreeMap::from([("v".to_owned(), Binding::LayerBelow)])
}

/// Graph `graph` playing from frame `from` to frame `to`.
fn item(graph: u32, from: usize, to: usize) -> RenderItem {
    RenderItem {
        graph,
        position: from as f64 / FPS,
        length: (to - from) as f64 / FPS,
        start: 0.0,
        pre_roll: true,
        bindings: below(),
    }
}

fn layer_set(layers: Vec<Vec<RenderItem>>, graphs: &[(u32, String)]) -> LayerSet {
    LayerSet {
        layers,
        graphs: graphs
            .iter()
            .map(|(id, json)| (*id, GraphDesc::from_json(json).unwrap()))
            .collect(),
        open: OPEN,
    }
}

fn renderer_for(set: Option<&LayerSet>, tracks: &[RenderTrack]) -> Renderer {
    Renderer::with_layers(
        &other_backend(),
        None,
        tracks,
        &GraphDesc::from_json(&passthrough()).unwrap(),
        set,
        Default::default(),
        &Bus::main(),
        &Registry::default(),
        OutputSize::Native,
    )
    .unwrap()
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

fn all_frames(r: &mut Renderer, frames: usize) -> Vec<Vec<u8>> {
    (0..frames)
        .map(|i| r.render(i, &|| false).unwrap().unwrap().to_vec())
        .collect()
}

/// A graph rendered over the whole timeline, the way projects without graph items are.
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
fn layers_without_items_show_the_track_mix() {
    let set = layer_set(vec![], &[]);
    let mut r = renderer_for(Some(&set), &video_track());
    assert_eq!(r.latency_frames(), 0);
    assert_eq!(all_frames(&mut r, 8), reference(&passthrough(), 8));
}

#[test]
fn an_item_changes_only_the_frames_it_plays() {
    let set = layer_set(vec![vec![item(1, 2, 5)]], &[(1, invert())]);
    let mut r = renderer_for(Some(&set), &video_track());
    let rendered = all_frames(&mut r, 8);
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
fn layers_compose_in_order() {
    // Inverted below, crushed above: the crush sees the inverted picture.
    let both = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "i", "type": "invert" }, { "id": "c", "type": "bitcrush", "params": { "bits": 2 } }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "i" }, { "from": "i", "to": "c" }, { "from": "c", "to": "o" }"#,
    );
    let set = layer_set(
        vec![vec![item(1, 1, 7)], vec![item(2, 3, 9)]],
        &[(1, invert()), (2, crush())],
    );
    let mut r = renderer_for(Some(&set), &video_track());
    let rendered = all_frames(&mut r, 10);
    let (plain, inverted, crushed, both) = (
        reference(&passthrough(), 10),
        reference(&invert(), 10),
        reference(&crush(), 10),
        reference(&both, 10),
    );
    assert_ne!(both[4], crushed[4]);
    for i in 0..10 {
        let expected = match ((1..7).contains(&i), (3..9).contains(&i)) {
            (false, false) => &plain,
            (true, false) => &inverted,
            (false, true) => &crushed,
            (true, true) => &both,
        };
        assert_eq!(rendered[i], expected[i], "frame {i}");
    }
}

#[test]
fn seeking_into_an_item_renders_what_playing_through_does() {
    let set = layer_set(
        vec![vec![item(1, 5, 40)], vec![item(2, 20, 50)]],
        &[(1, delay(2)), (2, delay(1))],
    );
    let expected = all_frames(&mut renderer_for(Some(&set), &video_track()), FRAMES);
    let mut r = renderer_for(Some(&set), &video_track());
    assert!(r.warmup_frames() >= 3, "both delays need history");
    let mut order: Vec<usize> = (0..FRAMES).rev().collect();
    order.extend((0..FRAMES).map(|i| (i * 37 + 11) % FRAMES));
    for i in order {
        let frame = r.render(i, &|| false).unwrap().unwrap();
        assert_eq!(frame, expected[i], "frame {i}");
    }
    // Inside the overlap, the upper delay saw the lower one's output: 3 frames behind.
    assert_eq!(expected[30][0], 27);
}

#[test]
fn inputs_read_the_track_they_are_bound_to_or_nothing() {
    // The `other` track plays 2 frames late.
    let mut other = RenderTrack::video("other", "other");
    other.items = vec![Item::whole(2.0 / FPS)];
    let tracks = tracks(vec![other]);

    let bound = RenderItem {
        bindings: BTreeMap::from([("v".to_owned(), Binding::Track("other".into()))]),
        ..item(1, 4, 8)
    };
    let unbound = RenderItem {
        bindings: BTreeMap::new(),
        ..item(1, 10, 12)
    };
    let set = layer_set(vec![vec![bound, unbound]], &[(1, passthrough())]);
    let mut r = renderer_for(Some(&set), &tracks);
    let rendered = all_frames(&mut r, 14);
    for (i, frame) in rendered.iter().enumerate() {
        match i {
            4..8 => assert_eq!(frame[0], (i - 2) as u8, "frame {i} shows `other`"),
            10..12 => assert!(frame.iter().all(|&b| b == 0), "frame {i} is black"),
            _ => assert_eq!(frame[0], i as u8, "frame {i} is the track mix"),
        }
    }
}

#[test]
fn pre_roll_warms_the_item_up_before_its_first_frame() {
    let first_pixel = |pre_roll: bool| {
        let set = layer_set(
            vec![vec![RenderItem {
                pre_roll,
                ..item(1, 6, 10)
            }]],
            &[(1, delay(2))],
        );
        let mut r = renderer_for(Some(&set), &video_track());
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
    // The frequency shifter delays by part of a frame, which is rounded up to a whole one.
    let shifted = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "s", "type": "frequency_shifter", "params": { "shift": 0, "mix": 0 } }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "s" }, { "from": "s", "to": "o" }"#,
    );
    let set = layer_set(
        vec![vec![item(1, 0, 4), item(2, 4, 8)]],
        &[(1, passthrough()), (2, shifted.clone())],
    );
    let mut r = renderer_for(Some(&set), &video_track());
    assert!(
        r.latency_frames() >= 1,
        "the layer takes its latest item's latency"
    );
    let rendered = all_frames(&mut r, 8);
    let plain = reference(&passthrough(), 8);
    let alone = reference(&shifted, 8);
    for i in 0..4 {
        assert_eq!(
            rendered[i], plain[i],
            "the quicker item is held back: frame {i}"
        );
    }
    for i in 4..8 {
        assert_eq!(rendered[i], alone[i], "frame {i}");
    }
}

#[test]
fn a_lower_layers_latency_shifts_the_layers_above_in_step() {
    let shifted = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "s", "type": "frequency_shifter", "params": { "shift": 0, "mix": 0 } }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "s" }, { "from": "s", "to": "o" }"#,
    );
    let both = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "s", "type": "frequency_shifter", "params": { "shift": 0, "mix": 0 } }, { "id": "i", "type": "invert" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "s" }, { "from": "s", "to": "i" }, { "from": "i", "to": "o" }"#,
    );
    let set = layer_set(
        vec![vec![item(1, 0, 12)], vec![item(2, 4, 9)]],
        &[(1, shifted.clone()), (2, invert())],
    );
    let mut r = renderer_for(Some(&set), &video_track());
    assert!(r.latency_frames() >= 1);
    let rendered = all_frames(&mut r, 12);
    let (alone, composed) = (reference(&shifted, 12), reference(&both, 12));
    for i in 0..12 {
        let expected = if (4..9).contains(&i) {
            &composed
        } else {
            &alone
        };
        assert_eq!(rendered[i], expected[i], "frame {i}");
    }
}

#[test]
fn taps_read_the_item_playing() {
    let set = layer_set(vec![vec![item(1, 2, 5)]], &[(1, invert())]);
    let mut r = renderer_for(Some(&set), &video_track());
    r.render(3, &|| false).unwrap();
    assert!(r.tap("i", 0).is_some());
    assert!(!r.levels().is_empty());
    assert!(!r.node_stats().is_empty());
    r.render(6, &|| false).unwrap();
    assert!(r.tap("i", 0).is_none(), "no item plays at frame 6");
    assert!(r.levels().is_empty());
}

#[test]
fn a_graph_that_cannot_compile_fails_the_build_naming_it() {
    let broken = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "nowhere" }"#,
    );
    let set = layer_set(vec![vec![item(7, 0, 4)]], &[(7, broken)]);
    let error = Renderer::with_layers(
        &other_backend(),
        None,
        &video_track(),
        &GraphDesc::from_json(&passthrough()).unwrap(),
        Some(&set),
        Default::default(),
        &Bus::main(),
        &Registry::default(),
        OutputSize::Native,
    )
    .unwrap_err();
    assert!(
        matches!(error, EngineError::Layer { graph: 7, .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains('7'), "{error}");
}

#[test]
fn only_the_items_that_play_supply_sound() {
    let sound = graph(
        r#"{ "id": "a", "type": "audio_input" }, { "id": "s", "type": "audio_output" }, { "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "a", "to": "s" }, { "from": "v", "to": "o" }"#,
    );
    let song = RenderTrack::audio(
        "song",
        Arc::new(rastersong_engine::sources::Modulator::new(&common::audio())),
        0.0,
    );
    let playing = RenderItem {
        bindings: BTreeMap::from([
            ("a".to_owned(), Binding::Track("song".into())),
            ("v".to_owned(), Binding::LayerBelow),
        ]),
        ..item(1, 3, 6)
    };
    let set = layer_set(vec![vec![playing]], &[(1, sound)]);
    let mut r = renderer_for(Some(&set), &tracks(vec![song]));
    assert!(matches!(r.audio_sink(), AudioSink::Rendered { .. }));
    let loud = |r: &mut Renderer, i: usize| {
        r.render(i, &|| false).unwrap();
        let block = r.audio().expect("every frame has a block").clone();
        block.samples.iter().any(|s| s.abs() > 0.01)
    };
    let rendered: Vec<bool> = (0..8).map(|i| loud(&mut r, i)).collect();
    assert_eq!(
        rendered,
        [false, false, false, true, true, true, false, false],
        "silent where no item plays"
    );
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
fn offline_renders_layers() {
    let set = layer_set(vec![vec![item(1, 2, 5)]], &[(1, invert())]);
    let settings = RenderSettings {
        frames: Some(8),
        layers: Some(set),
        ..Default::default()
    };
    let mut sink = Recorder::default();
    render(
        &other_backend(),
        &video_track(),
        &GraphDesc::from_json(&passthrough()).unwrap(),
        &settings,
        &mut sink,
    )
    .unwrap();
    let (plain, inverted) = (reference(&passthrough(), 8), reference(&invert(), 8));
    for i in 0..8 {
        let expected = if (2..5).contains(&i) {
            &inverted
        } else {
            &plain
        };
        assert_eq!(sink.0[i], expected[i], "frame {i}");
    }
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
fn the_service_renders_layers_and_they_are_part_of_the_cache_key() {
    let engine = engine();
    engine.set_timeline(Timeline {
        tracks: vec![TrackSpec::new(
            "video",
            TrackKind::Video,
            PathBuf::from(VIDEO),
        )],
        ..Timeline::default()
    });
    engine.set_graph(GraphDesc::from_json(&passthrough()).unwrap());
    assert_eq!(engine_frames(&engine, 8), reference(&passthrough(), 8));

    let set = layer_set(vec![vec![item(1, 2, 5)]], &[(1, invert())]);
    engine.set_layers(Some(set.clone()));
    let (plain, inverted) = (reference(&passthrough(), 8), reference(&invert(), 8));
    wait_until("the layers render", || {
        engine.frame(3).is_some_and(|f| f.rgb == inverted[3])
    });
    let rendered = engine_frames(&engine, 8);
    for i in 0..8 {
        let expected = if (2..5).contains(&i) {
            &inverted
        } else {
            &plain
        };
        assert_eq!(rendered[i], expected[i], "frame {i}");
    }
    assert_eq!(engine.status(), EngineStatus::Ready);

    // The same layers again, and edits to what the graphs don't render, keep the cache.
    let kept = engine.frame(3).unwrap();
    engine.set_layers(Some(set.clone()));
    let mut labelled = set.clone();
    labelled.graphs[0].1.nodes[0].label = Some("Picture".into());
    engine.set_layers(Some(labelled));
    assert!(Arc::ptr_eq(&kept, &engine.frame(3).unwrap()));

    // Bypassing the whole graph shows the track mix.
    engine.set_bypass_all(true);
    wait_until("the bypassed render", || {
        engine.frame(3).is_some_and(|f| f.rgb == plain[3])
    });

    // Removing the layers goes back to the open graph over the whole timeline.
    engine.set_bypass_all(false);
    engine.set_layers(None);
    wait_until("the open graph", || {
        engine.frame(3).is_some_and(|f| f.rgb == plain[3])
    });
}

#[test]
fn the_service_reports_a_graph_item_that_cannot_compile() {
    let engine = engine();
    engine.set_timeline(Timeline {
        tracks: vec![TrackSpec::new(
            "video",
            TrackKind::Video,
            PathBuf::from(VIDEO),
        )],
        ..Timeline::default()
    });
    engine.set_graph(GraphDesc::from_json(&passthrough()).unwrap());
    let broken = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "b", "type": "bogus" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "b" }, { "from": "b", "to": "o" }"#,
    );
    engine.set_layers(Some(layer_set(vec![vec![item(7, 0, 4)]], &[(7, broken)])));
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
