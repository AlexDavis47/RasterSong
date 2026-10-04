//! The background render service.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{AUDIO, FINITE, FRAMES, VIDEO, backend, crush, sequential, wait_until};
use rastersong_engine::{Engine, EngineConfig, EngineStatus, GraphDesc, OutputSize, PreviewScale};

fn engine() -> Engine {
    Engine::new(Arc::new(backend()), EngineConfig::default())
}

fn load(engine: &Engine, graph: &str) {
    engine.set_media(PathBuf::from(VIDEO), PathBuf::from(AUDIO));
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
    // Valid JSON, but `output` is missing its input.
    load(
        &engine,
        r#"{ "version": 1, "nodes": [ { "id": "out", "type": "output" } ] }"#,
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
