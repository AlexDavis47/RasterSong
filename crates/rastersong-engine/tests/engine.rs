//! The background render service.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{
    AUDIO, FINITE, FINITE_PASSTHROUGH, FRAMES, VIDEO, backend, crush, sequential, wait_until,
};
use rastersong_engine::{
    Engine, EngineConfig, EngineStatus, GraphDesc, Item, OutputSize, PreviewScale, Timeline,
    TrackKind, TrackSpec,
};

fn engine() -> Engine {
    Engine::new(Arc::new(backend()), EngineConfig::default())
}

/// The video, and an audio track of the song for each name, starting at the given time.
fn set_tracks(engine: &Engine, audio: &[(&str, f64)]) {
    let video = TrackSpec {
        name: "video".into(),
        kind: TrackKind::Video,
        path: PathBuf::from(VIDEO),
        items: vec![Item::whole(0.0)],
    };
    let tracks = audio.iter().map(|&(name, position)| TrackSpec {
        name: name.into(),
        kind: TrackKind::Audio,
        path: PathBuf::from(AUDIO),
        items: vec![Item::whole(position)],
    });
    engine.set_timeline(Timeline {
        timebase: None,
        tracks: std::iter::once(video).chain(tracks).collect(),
    });
}

fn load(engine: &Engine, graph: &str) {
    set_tracks(engine, &[("audio", 0.0)]);
    engine.set_graph(GraphDesc::from_json(graph).unwrap());
}

#[test]
fn renders_ahead_and_matches_a_full_render() {
    let engine = engine();
    assert_eq!(engine.status(), EngineStatus::Idle);
    load(&engine, FINITE);
    wait_until("all frames", || engine.buffered_from(0) == FRAMES);
    assert_eq!(engine.status(), EngineStatus::Ready);
    assert_eq!(engine.info().unwrap().frames, FRAMES);

    let expected = sequential(FINITE, OutputSize::Native);
    for (i, expected) in expected.iter().enumerate() {
        assert_eq!(&engine.frame(i).unwrap().rgb, expected, "frame {i}");
    }
}

#[test]
fn renders_from_the_playhead_after_a_seek() {
    let engine = engine();
    engine.set_playhead(40);
    load(&engine, FINITE);
    wait_until("frames from 40", || engine.buffered_from(40) == FRAMES - 40);
    assert!(
        engine.frame(0).is_none(),
        "nothing before the playhead was needed"
    );

    let expected = sequential(FINITE, OutputSize::Native);
    for (i, expected) in expected.iter().enumerate().skip(40) {
        assert_eq!(&engine.frame(i).unwrap().rgb, expected, "frame {i}");
    }
}

#[test]
fn edits_never_serve_stale_frames() {
    let engine = engine();
    load(&engine, &crush(2));
    wait_until("a few frames", || engine.buffered_from(0) >= 10);

    engine.set_graph(GraphDesc::from_json(&crush(6)).unwrap());
    let expected = sequential(&crush(6), OutputSize::Native);
    // From the moment the edit returns, every frame served must come from the new graph.
    let mut checked = 0;
    wait_until("the edited render", || {
        for (i, expected) in expected.iter().enumerate() {
            if let Some(frame) = engine.frame(i) {
                assert_eq!(&frame.rgb, expected, "stale frame {i} served after an edit");
                checked += 1;
            }
        }
        engine.buffered_from(0) == FRAMES
    });
    assert!(checked >= FRAMES);
}

#[test]
fn edits_to_nodes_that_do_not_feed_the_output_keep_the_cache() {
    let engine = engine();
    load(&engine, FINITE);
    wait_until("all frames", || engine.buffered_from(0) == FRAMES);
    let before = engine.frame(5).unwrap();

    // Add an orphan node: nothing rendered is invalidated, so the very same frames are served.
    let orphan = FINITE.replacen(
        r#""nodes": ["#,
        r#""nodes": [ { "id": "orphan", "type": "bitcrush", "position": [1, 2] },"#,
        1,
    );
    assert_ne!(orphan, FINITE, "the test graph's node list was found");
    engine.set_graph(GraphDesc::from_json(&orphan).unwrap());
    assert_eq!(engine.buffered_from(0), FRAMES);
    assert!(Arc::ptr_eq(&before, &engine.frame(5).unwrap()));

    // A change that matters renders again.
    engine.set_graph(GraphDesc::from_json(&crush(6)).unwrap());
    assert!(
        engine.frame(5).is_none_or(|f| !Arc::ptr_eq(&f, &before)),
        "the cache was dropped"
    );
}

#[test]
fn bypassing_the_whole_graph_renders_the_video() {
    let engine = engine();
    load(&engine, &crush(2));
    wait_until("processed frames", || engine.buffered_from(0) == FRAMES);
    engine.set_bypass_all(true);
    wait_until("bypassed frames", || engine.buffered_from(0) == FRAMES);
    assert_eq!(
        engine.frame(3).unwrap().rgb,
        sequential(FINITE_PASSTHROUGH, OutputSize::Native)[3]
    );
    engine.set_bypass_all(false);
    wait_until("processed again", || engine.buffered_from(0) == FRAMES);
    assert_eq!(
        engine.frame(3).unwrap().rgb,
        sequential(&crush(2), OutputSize::Native)[3]
    );
}

#[test]
fn preview_scale_renders_smaller_frames() {
    let engine = engine();
    load(&engine, FINITE);
    engine.set_preview_scale(PreviewScale::Half);
    wait_until("half-scale frames", || engine.buffered_from(0) == FRAMES);
    let frame = engine.frame(3).unwrap();
    assert_eq!((frame.width, frame.height), (8, 4));
    assert_eq!(&frame.rgb, &sequential(FINITE, OutputSize::Scaled(0.5))[3]);
}

#[test]
fn reports_errors_and_recovers() {
    let engine = engine();
    // Valid JSON, but the graph has no output node.
    load(
        &engine,
        r#"{ "version": 0, "nodes": [ { "id": "v", "type": "video_input" } ] }"#,
    );
    wait_until("the failure", || {
        matches!(engine.status(), EngineStatus::Failed(_))
    });

    engine.set_graph(GraphDesc::from_json(FINITE).unwrap());
    wait_until("recovery", || engine.buffered_from(0) == FRAMES);
    assert_eq!(engine.status(), EngineStatus::Ready);
}

#[test]
fn notifies_when_frames_arrive() {
    let engine = engine();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    engine.on_update({
        let count = count.clone();
        move || {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });
    load(&engine, FINITE);
    wait_until("notifications", || {
        count.load(std::sync::atomic::Ordering::SeqCst) > FRAMES
    });
}

#[test]
fn respects_the_cache_budget() {
    // Room for 20 frames of 16×8 RGB: the engine renders ahead only as far as fits.
    let engine = Engine::new(
        Arc::new(backend()),
        EngineConfig {
            cache_bytes: 20 * 16 * 8 * 3,
            lookahead_secs: 10.0,
        },
    );
    load(&engine, FINITE);
    wait_until("the window", || engine.buffered_from(0) == 15);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(
        engine.buffered_from(0),
        15,
        "renders 3/4 of the budget ahead and stops"
    );

    engine.set_playhead(30);
    wait_until("the next window", || engine.buffered_from(30) == 15);
}

#[test]
fn renders_without_audio_and_reports_cached_ranges() {
    let engine = engine();
    set_tracks(&engine, &[]);
    engine.set_graph(GraphDesc::from_json(FINITE).unwrap());
    wait_until("all frames", || engine.buffered_from(0) == FRAMES);
    assert_eq!(engine.cached_ranges(), vec![0..FRAMES]);
    assert!(engine.loaded_tracks().is_empty());
}

#[test]
fn audio_offset_changes_the_render_and_back() {
    let engine = engine();
    load(&engine, FINITE);
    wait_until("frames", || engine.buffered_from(0) == FRAMES);
    assert!((engine.loaded_tracks()[0].clip.duration_secs() - 2.0).abs() < 1e-9);
    let original = engine.frame(30).unwrap();

    set_tracks(&engine, &[("audio", 0.5)]);
    // The project runs to the end of the last item: the song now ends half a second later.
    wait_until("re-render", || engine.buffered_from(0) == FRAMES + 15);
    assert_eq!(engine.info().unwrap().frames, FRAMES + 15);
    assert_ne!(
        engine.frame(30).unwrap().rgb,
        original.rgb,
        "the offset moves the modulation"
    );

    set_tracks(&engine, &[("audio", 0.0)]);
    wait_until("re-render", || engine.buffered_from(0) == FRAMES);
    assert_eq!(
        engine.frame(30).unwrap().rgb,
        original.rgb,
        "and removing it restores the render"
    );
}

/// AM driven only by the `drums` track.
const DRUMS: &str = r#"{ "version": 0,
  "nodes": [
    { "id": "video", "type": "video_input" },
    { "id": "drums", "type": "audio_input", "params": { "source": "drums" } },
    { "id": "am", "type": "am", "params": { "depth": 2 } },
    { "id": "out", "type": "output" }
  ],
  "connections": [
    { "from": "video", "to": "am.carrier" }, { "from": "drums", "to": "am.modulator" }, { "from": "am", "to": "out" }
  ] }"#;

#[test]
fn audio_inputs_read_their_own_track_and_missing_tracks_are_silent() {
    let engine = engine();
    set_tracks(&engine, &[]);
    engine.set_graph(GraphDesc::from_json(DRUMS).unwrap());
    // Only an `audio` track: `drums` reads silence, so AM leaves the video unchanged.
    set_tracks(&engine, &[("audio", 0.0)]);
    wait_until("silent render", || engine.buffered_from(0) == FRAMES);
    assert_eq!(engine.status(), EngineStatus::Ready);
    let silent = engine.frame(20).unwrap();
    let plain = sequential(FINITE_PASSTHROUGH, OutputSize::Native);
    assert_eq!(silent.rgb, plain[20]);

    set_tracks(&engine, &[("audio", 0.0), ("drums", 0.0)]);
    wait_until("drums render", || engine.buffered_from(0) == FRAMES);
    assert_ne!(
        engine.frame(20).unwrap().rgb,
        silent.rgb,
        "the drums track now modulates"
    );
    assert_eq!(engine.loaded_tracks().len(), 2);
}

#[test]
fn failures_name_the_node_at_fault() {
    let engine = engine();
    load(
        &engine,
        r#"{ "version": 0, "nodes": [ { "id": "v", "type": "video_input" }, { "id": "c", "type": "pack", "params": { "channels": 7 } }, { "id": "o", "type": "output" } ],
            "connections": [ { "from": "v", "to": "c" }, { "from": "c", "to": "o" } ] }"#,
    );
    wait_until("the failure", || {
        matches!(engine.status(), EngineStatus::Failed(_))
    });
    let EngineStatus::Failed(failure) = engine.status() else {
        unreachable!()
    };
    assert_eq!(failure.node.as_deref(), Some("c"));
}

#[test]
fn frames_carry_output_levels() {
    let engine = engine();
    load(&engine, FINITE);
    wait_until("frames", || engine.buffered_from(0) >= 5);
    let frame = engine.frame(3).unwrap();
    for node in ["video", "audio", "delay", "crush", "out"] {
        assert!(
            frame.levels.iter().any(|l| &*l.node == node),
            "no level for {node}"
        );
    }
    assert!(
        frame
            .levels
            .iter()
            .all(|l| l.rms.is_finite() && l.rms >= 0.0)
    );
}

#[test]
fn graph_failures_keep_the_video_info() {
    let engine = engine();
    load(
        &engine,
        r#"{ "version": 0, "nodes": [ { "id": "v", "type": "video_input" } ] }"#,
    );
    wait_until("the failure", || {
        matches!(engine.status(), EngineStatus::Failed(_))
    });
    let info = engine.info().expect("the video's length is still known");
    assert_eq!(info.frames, FRAMES);
}

#[test]
fn cached_frames_carry_rendered_sound_that_playback_reads() {
    use rastersong_engine::AudioSink;
    use rastersong_engine::playback::Mixer;

    let engine = engine();
    load(
        &engine,
        r#"{ "version": 0,
          "nodes": [
            { "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
            { "id": "gain", "type": "gain", "params": { "gain": -6 } },
            { "id": "sound", "type": "audio_output" }, { "id": "out", "type": "output" }
          ],
          "connections": [
            { "from": "video", "to": "out" }, { "from": "audio", "to": "gain" }, { "from": "gain", "to": "sound" }
          ] }"#,
    );
    wait_until("frames", || engine.buffered_from(0) >= 10);
    assert_eq!(
        engine.audio_sink(),
        AudioSink::Rendered {
            sample_rate: 48_000,
            channels: 1
        }
    );
    let block = engine.frame(5).unwrap().audio.clone().unwrap();
    assert_eq!((block.start, block.frames()), (8_000, 1600));

    // Playback of the rendered sound: frames 2 to 4 at 30 fps, mixed to stereo at 48 kHz.
    let mixer = Mixer::rendered(engine.rendered_audio(), 1.0);
    let mut out = vec![0.0; 4800 * 2];
    mixer.render(2.0 / 30.0, 48_000.0, &mut out);
    let cached = engine.frame(2).unwrap().audio.clone().unwrap();
    for (j, pair) in out[..200].chunks(2).enumerate() {
        assert_eq!(pair[0], pair[1], "mono plays in both channels");
        assert!((pair[0] - cached.samples[j]).abs() < 1e-6, "sample {j}");
    }
}

mod taps {
    use super::*;
    use rastersong_engine::{TapOutcome, TapRequest};

    fn ask(engine: &Engine, request: &TapRequest) -> TapOutcome {
        let mut outcome = TapOutcome::Pending;
        wait_until("the tap's answer", || {
            outcome = engine.tap(request);
            outcome != TapOutcome::Pending
        });
        outcome
    }

    fn request(frame: usize, node: &str) -> TapRequest {
        TapRequest {
            frame,
            node: node.into(),
            output: 0,
        }
    }

    #[test]
    fn a_tap_reads_a_connection_without_disturbing_the_render() {
        let engine = engine();
        load(&engine, FINITE);
        wait_until("some frames", || engine.buffered_from(0) >= 10);

        let TapOutcome::Ready(tap) = ask(&engine, &request(7, "crush")) else {
            panic!("the crush node feeds the output");
        };
        // Crush is wired straight to the output, so its samples are the frame's pixels.
        let expected = engine.frame(7).map(|f| f.rgb.clone());
        let samples: Vec<u8> = tap
            .samples
            .as_deref()
            .unwrap()
            .iter()
            .map(|&x| (x.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
        if let Some(expected) = expected {
            assert_eq!(samples, expected);
        }
        assert!(tap.picture.is_some());

        // The cache fills as it would have without the tap.
        wait_until("all frames", || engine.buffered_from(0) == FRAMES);
        let reference = sequential(FINITE, OutputSize::Native);
        for (i, expected) in reference.iter().enumerate() {
            assert_eq!(&engine.frame(i).unwrap().rgb, expected, "frame {i}");
        }
    }

    #[test]
    fn a_connection_that_does_not_feed_the_output_is_not_rendered() {
        let engine = engine();
        let graph = FINITE
            .replace(
                r#"{ "id": "out", "type": "output" }"#,
                r#"{ "id": "out", "type": "output" }, { "id": "dead", "type": "bitcrush" }"#,
            )
            .replace(
                r#""connections": ["#,
                r#""connections": [ { "from": "video", "to": "dead" },"#,
            );
        load(&engine, &graph);
        wait_until("some frames", || engine.buffered_from(0) >= 2);
        assert_eq!(ask(&engine, &request(1, "dead")), TapOutcome::NotRendered);
        assert_eq!(
            ask(&engine, &request(1, "nothing")),
            TapOutcome::NotRendered
        );
    }

    #[test]
    fn an_edit_drops_the_answer() {
        let engine = engine();
        load(&engine, &crush(2));
        let asked = request(3, "crush");
        assert!(matches!(ask(&engine, &asked), TapOutcome::Ready(_)));
        assert!(
            matches!(engine.tap(&asked), TapOutcome::Ready(_)),
            "kept until edited"
        );

        engine.set_graph(GraphDesc::from_json(&crush(6)).unwrap());
        assert_eq!(engine.tap(&asked), TapOutcome::Pending);
        let TapOutcome::Ready(tap) = ask(&engine, &asked) else {
            panic!("expected a new answer");
        };
        let new = sequential(&crush(6), OutputSize::Native);
        let samples: Vec<u8> = tap
            .samples
            .as_deref()
            .unwrap()
            .iter()
            .map(|&x| (x.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
        assert_eq!(samples, new[3]);
    }
}

mod listening {
    use super::*;
    use rastersong_engine::ListenTarget;

    fn target(node: &str) -> ListenTarget {
        ListenTarget {
            node: node.into(),
            output: 0,
        }
    }

    #[test]
    fn a_connection_is_heard_ahead_of_the_position_without_disturbing_the_render() {
        let engine = engine();
        load(&engine, FINITE);
        wait_until("some frames", || engine.buffered_from(0) >= 5);
        let source = engine.listened_audio();

        engine.listen(Some(target("audio")), 3);
        wait_until("the sound", || {
            engine.listen(Some(target("audio")), 3);
            source.blocks(3..8).iter().all(|b| b.is_some())
        });
        let block = source.blocks(4..5)[0].clone().unwrap();
        assert!(!block.samples.is_empty());
        assert!(block.samples.iter().any(|&x| x.abs() > 0.01), "silent");

        // The sound moves on with the position.
        wait_until("the sound at 50", || {
            engine.listen(Some(target("audio")), 50);
            source.blocks(50..55).iter().all(|b| b.is_some())
        });
        assert!(source.blocks(3..4)[0].is_none(), "old sound is dropped");

        // Stopping clears it, and the cache is as it would have been.
        engine.listen(None, 50);
        assert!(source.blocks(50..55).iter().all(|b| b.is_none()));
        wait_until("all frames", || engine.buffered_from(0) == FRAMES);
        let reference = sequential(FINITE, OutputSize::Native);
        for (i, expected) in reference.iter().enumerate() {
            assert_eq!(&engine.frame(i).unwrap().rgb, expected, "frame {i}");
        }
    }

    #[test]
    fn a_connection_that_is_not_rendered_is_silent() {
        let engine = engine();
        load(&engine, FINITE);
        let source = engine.listened_audio();
        wait_until("silence", || {
            engine.listen(Some(target("nothing")), 2);
            source.blocks(2..4).iter().all(|b| b.is_some())
        });
        assert!(
            source
                .blocks(2..4)
                .iter()
                .flatten()
                .all(|b| b.samples.is_empty())
        );
    }
}
