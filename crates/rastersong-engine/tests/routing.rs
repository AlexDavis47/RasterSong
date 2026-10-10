//! Routing: FX chains on items, tracks, folders and the master, and receives between tracks,
//! rendered by the sequential renderer, the service and the offline render.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use common::{FRAMES, VIDEO, backend, tracks, wait_until};
use rastersong_engine::{
    AudioSink, Bus, Engine, EngineConfig, EngineError, EngineStatus, FakeVideo, FrameSink, Fx,
    GraphDesc, Item, OutputSize, Rational, Registry, RenderInfo, RenderSettings, RenderTrack,
    RenderedFrame, Renderer, RouteTrack, Routing, Timeline, TrackKind, TrackSpec, render,
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

/// The clip's frames `from..to`, where they are on the timeline, with FX `fx`.
fn item(from: usize, to: usize, fx: Vec<Fx>) -> Item {
    Item {
        start: from as f64 / FPS,
        end: Some(to as f64 / FPS),
        fx,
        ..Item::whole(from as f64 / FPS)
    }
}

/// The video track cut at `cuts`, each `(from, to, graph)` playing an FX of `graph`; the rest of
/// the clip plays plain.
fn cut_video(cuts: &[(usize, usize, u32)]) -> Vec<RenderTrack> {
    let mut items = Vec::new();
    let mut at = 0;
    for &(from, to, graph) in cuts {
        if from > at {
            items.push(item(at, from, Vec::new()));
        }
        items.push(item(from, to, vec![Fx::new(graph)]));
        at = to;
    }
    if at < FRAMES {
        items.push(item(at, FRAMES, Vec::new()));
    }
    let mut video = tracks(Vec::new());
    video[0].items = items;
    video
}

/// Every track of `tracks` at the top, with no FX.
fn routing(tracks: &[RenderTrack], graphs: &[(u32, String)]) -> Routing {
    Routing {
        tracks: tracks
            .iter()
            .map(|t| {
                let kind = match t.media {
                    rastersong_engine::TrackMedia::Video { .. } => TrackKind::Video,
                    rastersong_engine::TrackMedia::Audio(_) => TrackKind::Audio,
                };
                RouteTrack::new(t.name.clone(), Some(kind))
            })
            .collect(),
        master_fx: Vec::new(),
        graphs: graphs
            .iter()
            .map(|(id, json)| (*id, GraphDesc::from_json(json).unwrap()))
            .collect(),
        open: OPEN,
    }
}

fn build(routing: &Routing, tracks: &[RenderTrack]) -> Result<Renderer, EngineError> {
    Renderer::with_routing(
        &other_backend(),
        None,
        tracks,
        routing,
        &GraphDesc::from_json(&passthrough()).unwrap(),
        Default::default(),
        &Bus::main(),
        &Registry::default(),
        OutputSize::Native,
    )
}

fn renderer_for(routing: &Routing, tracks: &[RenderTrack]) -> Renderer {
    build(routing, tracks).unwrap()
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

/// A graph rendered over the video track as the master's FX.
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
fn without_fx_the_track_mix_shows() {
    let tracks = video_track();
    let mut r = renderer_for(&routing(&tracks, &[]), &tracks);
    assert_eq!(r.latency_frames(), 0);
    assert!(matches!(r.audio_sink(), AudioSink::TrackMix));
    assert_eq!(all_frames(&mut r, 8), reference(&passthrough(), 8));
}

#[test]
fn an_item_fx_changes_only_the_frames_its_item_plays() {
    let tracks = cut_video(&[(2, 5, 1)]);
    let mut r = renderer_for(&routing(&tracks, &[(1, invert())]), &tracks);
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
fn chains_run_in_order_items_then_tracks_then_the_master() {
    // Inverted on the item, crushed on the track: the crush sees the inverted picture.
    let both = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "i", "type": "invert" }, { "id": "c", "type": "bitcrush", "params": { "bits": 2 } }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "i" }, { "from": "i", "to": "c" }, { "from": "c", "to": "o" }"#,
    );
    let tracks = cut_video(&[(1, 7, 1)]);
    let mut on_track = routing(&tracks, &[(1, invert()), (2, crush())]);
    on_track.tracks[0].fx = vec![Fx::new(2)];
    let mut on_master = routing(&tracks, &[(1, invert()), (2, crush())]);
    on_master.master_fx = vec![Fx::new(2)];
    let (inverted, crushed, both) = (
        reference(&invert(), 10),
        reference(&crush(), 10),
        reference(&both, 10),
    );
    assert_ne!(both[4], crushed[4]);
    assert_ne!(both[4], inverted[4]);
    for routing in [on_track, on_master] {
        let rendered = all_frames(&mut renderer_for(&routing, &tracks), 10);
        for i in 0..10 {
            let expected = if (1..7).contains(&i) { &both } else { &crushed };
            assert_eq!(rendered[i], expected[i], "frame {i}");
        }
    }

    // A bypassed FX is left out.
    let mut bypassed = routing(&tracks, &[(1, invert()), (2, crush())]);
    bypassed.master_fx = vec![Fx {
        bypass: true,
        ..Fx::new(2)
    }];
    let rendered = all_frames(&mut renderer_for(&bypassed, &tracks), 10);
    assert_eq!(rendered[0], reference(&passthrough(), 1)[0]);
    assert_eq!(rendered[4], inverted[4]);
}

#[test]
fn seeking_into_an_item_renders_what_playing_through_does() {
    let tracks = cut_video(&[(5, 40, 1)]);
    let mut routing = routing(&tracks, &[(1, delay(2)), (2, delay(1))]);
    routing.master_fx = vec![Fx::new(2)];
    let expected = all_frames(&mut renderer_for(&routing, &tracks), FRAMES);
    let mut r = renderer_for(&routing, &tracks);
    assert!(r.warmup_frames() >= 3, "both delays need history");
    let mut order: Vec<usize> = (0..FRAMES).rev().collect();
    order.extend((0..FRAMES).map(|i| (i * 37 + 11) % FRAMES));
    for i in order {
        let frame = r.render(i, &|| false).unwrap().unwrap();
        assert_eq!(frame, expected[i], "frame {i}");
    }
    // Inside the item, the master's delay saw the item's: 3 frames behind.
    assert_eq!(expected[30][0], 27);
}

#[test]
fn ports_read_the_track_they_receive_from_or_nothing() {
    // `other` sends nowhere: it is only there to be received.
    let tracks = tracks(vec![late("other")]);
    let mut routing = routing(&tracks, &[(1, show("Other"))]);
    routing.tracks[1].master_send = false;
    routing.master_fx = vec![Fx {
        receives: BTreeMap::from([("Other".to_owned(), "other".to_owned())]),
        ..Fx::new(1)
    }];
    let rendered = all_frames(&mut renderer_for(&routing, &tracks), 8);
    for (i, frame) in rendered.iter().enumerate().skip(2) {
        assert_eq!(frame[0], (i - 2) as u8, "frame {i} shows `other`");
    }

    // Without the receive the port reads zeros.
    routing.master_fx[0].receives.clear();
    let rendered = all_frames(&mut renderer_for(&routing, &tracks), 8);
    assert!(rendered.iter().flatten().all(|&b| b == 0));

    // And the track sending nowhere doesn't show without FX either.
    routing.master_fx.clear();
    let rendered = all_frames(&mut renderer_for(&routing, &tracks), 8);
    assert_eq!(rendered, reference(&passthrough(), 8));
}

#[test]
fn a_receive_with_latency_holds_the_rest_back_to_line_up() {
    // `other` comes a frame late through its FX; the master's graph reads it beside the main
    // picture, which is held back a frame to meet it.
    let tracks = tracks(vec![late("other")]);
    let side_by_side = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "x", "type": "video_input", "params": { "port": "Other" } }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "o" }"#,
    );
    let mut routing = routing(
        &tracks,
        &[(1, shifted()), (2, side_by_side), (3, show("Other"))],
    );
    routing.tracks[1].fx = vec![Fx::new(1)];
    routing.tracks[1].master_send = false;
    let receive = BTreeMap::from([("Other".to_owned(), "other".to_owned())]);
    for (graph, expect) in [(2, 0), (3, 2)] {
        routing.master_fx = vec![Fx {
            receives: receive.clone(),
            ..Fx::new(graph)
        }];
        let mut r = renderer_for(&routing, &tracks);
        assert_eq!(r.latency_frames(), 1, "graph {graph}");
        let rendered = all_frames(&mut r, 10);
        for (i, frame) in rendered.iter().enumerate().skip(expect) {
            assert_eq!(frame[0], (i - expect) as u8, "graph {graph}, frame {i}");
        }
    }
}

#[test]
fn receives_that_make_a_loop_are_refused() {
    let tracks = tracks(vec![late("other")]);
    let mut routing = routing(&tracks, &[(1, show("Other"))]);
    let from = |track: &str| Fx {
        receives: BTreeMap::from([("Other".to_owned(), track.to_owned())]),
        ..Fx::new(1)
    };
    routing.tracks[0].fx = vec![from("other")];
    routing.tracks[1].fx = vec![from("Video")];
    let error = build(&routing, &tracks).unwrap_err();
    assert!(matches!(error, EngineError::Routing(_)), "{error:?}");
    assert!(error.to_string().contains("Video"), "{error}");

    // A track receiving from itself is a loop too.
    routing.tracks[1].fx.clear();
    routing.tracks[0].fx = vec![from("Video")];
    assert!(matches!(
        build(&routing, &tracks).unwrap_err(),
        EngineError::Routing(_)
    ));
}

#[test]
fn a_folder_shows_its_top_track_playing_through_its_own_fx() {
    // `top` plays half a second of the clip from 1 s, over `bottom` playing all of it, in a
    // folder that inverts them; `other` is below the folder, 2 frames late.
    let top = RenderTrack {
        items: vec![Item {
            end: Some(0.5),
            ..Item::whole(1.0)
        }],
        ..RenderTrack::video("top", VIDEO)
    };
    let tracks = vec![top, RenderTrack::video("bottom", VIDEO), late("other")];
    let mut routing = routing(&tracks, &[(1, invert())]);
    let folder = RouteTrack {
        folder: true,
        fx: vec![Fx::new(1)],
        ..RouteTrack::new("Folder", None)
    };
    routing.tracks.insert(0, folder);
    routing.tracks[1].depth = 1;
    routing.tracks[2].depth = 1;
    let inverted = reference(&invert(), FRAMES);
    let rendered = all_frames(&mut renderer_for(&routing, &tracks), FRAMES);
    for (i, frame) in rendered.iter().enumerate() {
        let shown = if (30..45).contains(&i) { i - 30 } else { i };
        assert_eq!(*frame, inverted[shown], "frame {i}");
    }

    // Muting the folder shows what is below it.
    routing.tracks[0].muted = true;
    let rendered = all_frames(&mut renderer_for(&routing, &tracks), 8);
    for (i, frame) in rendered.iter().enumerate().skip(2) {
        assert_eq!(frame[0], (i - 2) as u8, "frame {i}");
    }
}

#[test]
fn pre_roll_warms_an_item_up_before_its_first_frame() {
    let first_pixel = |pre_roll: bool| {
        let mut tracks = cut_video(&[(6, 10, 1)]);
        tracks[0].items[1].pre_roll = pre_roll;
        let mut r = renderer_for(&routing(&tracks, &[(1, delay(2))]), &tracks);
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
    let tracks = cut_video(&[(0, 4, 1), (4, 8, 2)]);
    let mut r = renderer_for(
        &routing(&tracks, &[(1, passthrough()), (2, shifted())]),
        &tracks,
    );
    assert_eq!(
        r.latency_frames(),
        1,
        "the track takes its slowest item's latency"
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
        "after the items, the clip plays on in step"
    );
}

#[test]
fn taps_read_the_fx_that_ran() {
    let tracks = cut_video(&[(2, 5, 1)]);
    let mut r = renderer_for(&routing(&tracks, &[(1, invert())]), &tracks);
    r.render(3, &|| false).unwrap();
    assert!(r.tap("i", 0).is_some());
    assert!(!r.levels().is_empty());
    assert!(!r.node_stats().is_empty());
    r.render(6, &|| false).unwrap();
    assert!(r.tap("i", 0).is_none(), "no FX ran at frame 6");
    assert!(r.levels().is_empty());
}

#[test]
fn a_graph_that_cannot_compile_fails_the_build_naming_it() {
    let broken = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "nowhere" }"#,
    );
    let tracks = video_track();
    let mut routing = routing(&tracks, &[(7, broken)]);
    routing.tracks[0].fx = vec![Fx::new(7)];
    let error = build(&routing, &tracks).unwrap_err();
    assert!(
        matches!(error, EngineError::Fx { graph: 7, .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains('7'), "{error}");
}

#[test]
fn rendered_sound_is_the_tracks_mixed_at_their_volume() {
    let sound = graph(
        r#"{ "id": "a", "type": "audio_input" }, { "id": "s", "type": "audio_output" }, { "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "a", "to": "s" }, { "from": "v", "to": "o" }"#,
    );
    let song = RenderTrack::audio(
        "song",
        Arc::new(rastersong_engine::sources::Modulator::new(&common::audio())),
        0.0,
    );
    let tracks = tracks(vec![song]);
    let peak = |volume: f32, muted: bool| {
        let mut routing = routing(&tracks, &[(1, sound.clone())]);
        routing.master_fx = vec![Fx::new(1)];
        routing.tracks[1].volume = volume;
        routing.tracks[1].muted = muted;
        let mut r = renderer_for(&routing, &tracks);
        assert!(matches!(r.audio_sink(), AudioSink::Rendered { .. }));
        (0..8)
            .flat_map(|i| {
                r.render(i, &|| false).unwrap();
                r.audio().expect("every frame has a block").samples.clone()
            })
            .fold(0.0f32, |m, s| m.max(s.abs()))
    };
    let (full, half) = (peak(1.0, false), peak(0.5, false));
    assert!(full > 0.1, "{full}");
    assert!((half / full - 0.5).abs() < 0.01, "{half} / {full}");
    assert!(peak(1.0, true) < 1e-6, "a muted track is silent");
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
fn offline_renders_the_routing() {
    let tracks = video_track();
    let mut routing = routing(&tracks, &[(1, invert())]);
    routing.tracks[0].fx = vec![Fx::new(1)];
    let settings = RenderSettings {
        frames: Some(8),
        routing: Some(routing),
        ..Default::default()
    };
    let mut sink = Recorder::default();
    render(
        &other_backend(),
        &tracks,
        &GraphDesc::from_json(&passthrough()).unwrap(),
        &settings,
        &mut sink,
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
            PathBuf::from(VIDEO),
        )],
        ..Timeline::default()
    }
}

#[test]
fn the_service_renders_the_routing_and_it_is_part_of_the_cache_key() {
    let engine = engine();
    engine.set_timeline(video_timeline());
    engine.set_graph(GraphDesc::from_json(&passthrough()).unwrap());
    let (plain, inverted) = (reference(&passthrough(), 8), reference(&invert(), 8));
    assert_eq!(engine_frames(&engine, 8), plain);

    let mut routing = routing(&video_track(), &[(1, invert())]);
    routing.tracks[0].fx = vec![Fx::new(1)];
    engine.set_routing(Some(routing.clone()));
    wait_until("the routing renders", || {
        engine.frame(3).is_some_and(|f| f.rgb == inverted[3])
    });
    assert_eq!(engine_frames(&engine, 8), inverted);
    assert_eq!(engine.status(), EngineStatus::Ready);

    // The same routing again, and edits to what the graphs don't render, keep the cache.
    let kept = engine.frame(3).unwrap();
    engine.set_routing(Some(routing.clone()));
    let mut labelled = routing.clone();
    labelled.graphs[0].1.nodes[0].label = Some("Picture".into());
    engine.set_routing(Some(labelled));
    assert!(Arc::ptr_eq(&kept, &engine.frame(3).unwrap()));

    // Bypassing every FX shows the track mix.
    engine.set_bypass_all(true);
    wait_until("the bypassed render", || {
        engine.frame(3).is_some_and(|f| f.rgb == plain[3])
    });

    // An item's FX come with the timeline.
    engine.set_bypass_all(false);
    let mut timeline = video_timeline();
    timeline.tracks[0].items = vec![item(0, 2, Vec::new()), item(2, FRAMES, vec![Fx::new(1)])];
    engine.set_routing(Some(Routing {
        tracks: vec![RouteTrack::new("Video", Some(TrackKind::Video))],
        ..routing
    }));
    engine.set_timeline(timeline);
    wait_until("the item's FX", || {
        engine.frame(3).is_some_and(|f| f.rgb == inverted[3])
    });
    assert_eq!(engine.frame(1).unwrap().rgb, plain[1]);

    // Without a routing the open graph is the master's FX.
    engine.set_routing(None);
    engine.set_timeline(video_timeline());
    wait_until("the open graph", || {
        engine.frame(3).is_some_and(|f| f.rgb == plain[3])
    });
}

#[test]
fn the_service_reports_an_fx_that_cannot_compile() {
    let engine = engine();
    engine.set_timeline(video_timeline());
    engine.set_graph(GraphDesc::from_json(&passthrough()).unwrap());
    let broken = graph(
        r#"{ "id": "v", "type": "video_input" }, { "id": "b", "type": "bogus" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "b" }, { "from": "b", "to": "o" }"#,
    );
    let mut routing = routing(&video_track(), &[(7, broken)]);
    routing.master_fx = vec![Fx::new(7)];
    engine.set_routing(Some(routing));
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
