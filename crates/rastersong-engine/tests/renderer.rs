//! Seeking, warmup, latency and cancellation in the sequential renderer.

mod common;

use std::cell::Cell;

use common::{FINITE, FRAMES, INFINITE, renderer, renderer_with, sequential};
use rastersong_engine::{OutputSize, Registry};

/// Deterministic jumps around the video: backwards, far forwards, small hops.
fn jumpy_order() -> Vec<usize> {
    let mut order: Vec<usize> = (0..FRAMES).rev().collect();
    order.extend((0..FRAMES).map(|i| (i * 37 + 11) % FRAMES));
    order.extend([5, 6, 7, 30, 31, 2, 59, 0]);
    order
}

#[test]
fn seeking_is_exact_for_finite_memory_graphs() {
    let expected = sequential(FINITE, OutputSize::Native);
    let mut r = renderer(FINITE);
    assert!(r.warmup_frames() >= 2, "the delay needs history");
    for i in jumpy_order() {
        let frame = r.render(i, &|| false).unwrap().unwrap();
        assert_eq!(frame, expected[i], "frame {i}");
    }
}

#[test]
fn seeking_is_close_for_infinite_memory_graphs() {
    let expected = sequential(INFINITE, OutputSize::Native);
    let mut r = renderer(INFINITE);
    for i in jumpy_order() {
        let frame = r.render(i, &|| false).unwrap().unwrap();
        let worst = frame
            .iter()
            .zip(&expected[i])
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(worst <= 2, "frame {i} differs by up to {worst} levels");
    }
}

#[test]
fn scaled_output_is_smaller() {
    let mut r = renderer_with(FINITE, &Registry::default(), OutputSize::Scaled(0.5));
    assert_eq!((r.info().width, r.info().height), (8, 4));
    assert_eq!(r.render(0, &|| false).unwrap().unwrap().len(), 8 * 4 * 3);
}

#[test]
fn cancelling_leaves_the_renderer_consistent() {
    let expected = sequential(FINITE, OutputSize::Native);
    let mut r = renderer(FINITE);
    // Cancel partway through the warmup for frame 30.
    let calls = Cell::new(0);
    let cancel = || {
        calls.set(calls.get() + 1);
        calls.get() > 1
    };
    assert!(r.render(30, &cancel).unwrap().is_none());
    assert_eq!(r.render(30, &|| false).unwrap().unwrap(), expected[30]);
    assert_eq!(r.render(31, &|| false).unwrap().unwrap(), expected[31]);
}

#[test]
fn rendering_reports_the_warmup_it_does_after_a_seek() {
    let mut r = renderer(FINITE);
    let warmup = r.warmup_frames();
    assert!(warmup >= 2);
    let steps = std::cell::RefCell::new(Vec::new());
    let record = |step| steps.borrow_mut().push(step);

    // A fresh seek processes the history first, then the frame itself.
    r.render_with(30, &|| false, &record).unwrap().unwrap();
    {
        let steps = steps.borrow();
        assert_eq!(steps.len(), warmup + 1);
        assert!(steps.iter().all(|s| s.restarted && s.total == warmup + 1));
        assert_eq!(steps.first().unwrap().done, 0);
        assert_eq!(steps.last().unwrap().done, warmup);
    }

    // The next frame carries on: one step, no warm-up.
    steps.borrow_mut().clear();
    r.render_with(31, &|| false, &record).unwrap().unwrap();
    let steps = steps.borrow();
    assert_eq!(steps.len(), 1);
    assert!(!steps[0].restarted && steps[0].total == 1);
}

#[test]
fn out_of_range_frames_are_an_error() {
    assert!(renderer(FINITE).render(FRAMES, &|| false).is_err());
}

const LATENCY_GRAPH: &str = r#"{ "version": 1,
  "nodes": [
    { "id": "video", "type": "video_input" }, { "id": "look", "type": "lookahead" },
    { "id": "sum", "type": "sum" }, { "id": "half", "type": "am", "params": { "depth": -0.5 } },
    { "id": "out", "type": "output" }
  ],
  "connections": [
    { "from": "video", "to": "look" }, { "from": "look", "to": "sum.a" }, { "from": "video", "to": "sum.b" },
    { "from": "sum", "to": "half.carrier" }, { "from": "video", "to": "half.modulator" }, { "from": "half", "to": "out" }
  ] }"#;

#[test]
fn latency_is_compensated_across_seeks() {
    let mut registry = Registry::default();
    rastersong_graph::testing::register_fakes(&mut registry);

    // Both branches of `sum` carry the same frame once compensated, and the output is shifted back
    // by the graph's latency, so output frame i is source frame i doubled. (The `am` node's
    // modulator input is compensated too; with depth -0.5 and modulator = video it computes
    // 2v × (1 - v/2), which only matches if every input is aligned.)
    let mut r = renderer_with(LATENCY_GRAPH, &registry, OutputSize::Native);
    assert_eq!(r.latency_frames(), 1);
    for i in jumpy_order() {
        let frame = r.render(i, &|| false).unwrap().unwrap();
        // The fake video's red channel is the frame index.
        let v = i as f32 / 255.0;
        let expected = (2.0 * v * (1.0 - 0.5 * v) * 255.0).round() as u8;
        assert_eq!(frame[0], expected, "frame {i}");
    }
}
