//! Seeking, warmup, latency and cancellation in the sequential renderer.

mod common;

use std::cell::Cell;
use std::sync::Arc;

use common::{FINITE, FRAMES, INFINITE, renderer, renderer_with, sequential};
use rastersong_engine::{OutputSize, Registry, RenderTrack};

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

/// Generators set to the audio layout work whatever the project's audio tracks are called, and
/// even when it has none.
#[test]
fn the_audio_layout_exists_without_a_track_named_audio() {
    use common::{AUDIO, backend};
    use rastersong_engine::sources::Modulator;
    use rastersong_engine::{GraphDesc, RenderTrack};
    use std::sync::Arc;
    let graph = GraphDesc::from_json(
        r#"{ "version": 0,
          "nodes": [
            { "id": "video", "type": "video_input" },
            { "id": "beat", "type": "beat", "layout": "audio", "params": { "shape": "phase" } },
            { "id": "crush", "type": "bitcrush" },
            { "id": "out", "type": "output" }
          ],
          "connections": [
            { "from": "video", "to": "crush" }, { "from": "beat", "to": "crush.@bits" },
            { "from": "crush", "to": "out" }
          ] }"#,
    )
    .unwrap();
    let song = || RenderTrack::audio(AUDIO, Arc::new(Modulator::new(&common::audio())), 0.0);
    for tracks in [vec![song()], vec![]] {
        let mut r = rastersong_engine::Renderer::new(
            &backend(),
            None,
            &common::tracks(tracks),
            &graph,
            Default::default(),
            &rastersong_engine::Bus::main(),
            &Registry::default(),
            OutputSize::Native,
        )
        .expect("compiles with an audio layout");
        assert!(r.render(3, &|| false).unwrap().is_some());
    }
}

#[test]
fn the_pixel_unit_scales_with_the_preview() {
    let render = |size| renderer_with(FINITE, &Registry::default(), size);
    assert_eq!(
        render(OutputSize::Native).compile_options().pixel_scale,
        1.0
    );
    assert_eq!(
        render(OutputSize::Scaled(0.5))
            .compile_options()
            .pixel_scale,
        0.5
    );
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

const LATENCY_GRAPH: &str = r#"{ "version": 0,
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

#[test]
fn stereo_tracks_reach_the_graph_interleaved() {
    use std::sync::Arc;

    use rastersong_engine::sources::Modulator;
    use rastersong_engine::{AudioClip, ChannelMap, GraphDesc, Kind, RenderTrack, Renderer};

    // Left is a constant 0.5, right -0.5; the graph splits off the right and shows its level.
    let clip = AudioClip {
        sample_rate: 9000,
        channels: 2,
        samples: [0.5, -0.5].repeat(9000 * 3).into(),
    };
    let graph = r#"{ "version": 0,
      "nodes": [
        { "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
        { "id": "split", "type": "split" }, { "id": "am", "type": "am" }, { "id": "out", "type": "output" }
      ],
      "connections": [
        { "from": "audio", "to": "split" }, { "from": "video", "to": "am.carrier" },
        { "from": "split.c2", "to": "am.modulator" }, { "from": "am", "to": "out" }
      ] }"#;
    let mut r = Renderer::new(
        &common::backend(),
        None,
        &common::tracks(vec![RenderTrack::audio(
            "audio",
            Arc::new(Modulator::new(&clip)),
            0.0,
        )]),
        &GraphDesc::from_json(graph).unwrap(),
        Default::default(),
        &rastersong_engine::Bus::main(),
        &Registry::default(),
        OutputSize::Native,
    )
    .unwrap();
    let audio = &r
        .node_stats()
        .iter()
        .find(|s| &*s.node == "audio")
        .unwrap()
        .outputs[0];
    assert_eq!(audio.samples_per_pixel, 2);
    assert_eq!(
        (audio.tag.kind, audio.tag.channels),
        (Kind::Audio, ChannelMap::Stereo)
    );
    r.render(10, &|| false).unwrap();
    let right = r
        .levels()
        .into_iter()
        .find(|l| &*l.node == "split" && l.output == 1)
        .unwrap();
    assert!((right.rms - 0.5).abs() < 1e-6, "{}", right.rms);
}

/// The test video with its audio inverted into an Audio Output.
const SOUND: &str = r#"{ "version": 0,
  "nodes": [
    { "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
    { "id": "inv", "type": "invert", "params": { "mode": "audio" } },
    { "id": "sound", "type": "audio_output" }, { "id": "out", "type": "output" }
  ],
  "connections": [
    { "from": "video", "to": "out" }, { "from": "audio", "to": "inv" }, { "from": "inv", "to": "sound" }
  ] }"#;

#[test]
fn an_audio_output_renders_sound_with_each_frame() {
    use rastersong_engine::AudioSink;

    let mut r = renderer(SOUND);
    assert_eq!(
        r.audio_sink(),
        AudioSink::Rendered {
            sample_rate: 48_000,
            channels: 2
        }
    );
    // 30 fps: exactly 1600 samples a frame at 48 kHz, one block after another.
    let mut blocks = Vec::new();
    for i in 0..6 {
        r.render(i, &|| false).unwrap();
        blocks.push(r.audio().unwrap().clone());
    }
    for (i, b) in blocks.iter().enumerate() {
        assert_eq!((b.start, b.frames()), (i as u64 * 1600, 1600));
    }
    // The source is a sine at 0.8; inverted and resampled, it's still a full-level wave.
    let peak = blocks[3].samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!((0.7..=0.81).contains(&peak), "{peak}");

    // After a seek (with its warm-up), the same frame has the same sound.
    let mut seeking = renderer(SOUND);
    seeking.render(30, &|| false).unwrap();
    seeking.render(4, &|| false).unwrap();
    assert_eq!(seeking.audio().unwrap(), &blocks[4]);

    // Another rate.
    r.set_audio_rate(24_000);
    r.render(2, &|| false).unwrap();
    assert_eq!(r.audio().unwrap().frames(), 800);
}

#[test]
fn a_track_wired_straight_to_the_audio_output_is_passed_through() {
    use rastersong_engine::AudioSink;

    let graph = SOUND.replace(
        r#"{ "from": "audio", "to": "inv" }, { "from": "inv", "to": "sound" }"#,
        r#"{ "from": "audio", "to": "sound" }"#,
    );
    let mut r = renderer(&graph);
    assert_eq!(r.audio_sink(), AudioSink::Passthrough("audio".into()));
    r.render(0, &|| false).unwrap();
    assert!(r.audio().is_none());
    assert_eq!(renderer(common::FINITE).audio_sink(), AudioSink::TrackMix);
}

#[test]
fn tracks_at_different_rates_meet_in_one_graph() {
    use std::sync::Arc;

    use rastersong_engine::sources::Modulator;
    use rastersong_engine::{AudioClip, AudioSink, GraphDesc, RenderTrack, Renderer, Severity};

    // A 9 kHz mono track and a 16 kHz stereo one, combined into the audio output: their blocks
    // differ in length (300 and 533 frames at 30 fps), so the second is stretched to the first.
    let mono = AudioClip {
        sample_rate: 9_000,
        channels: 1,
        samples: vec![0.25; 9_000 * 3].into(),
    };
    let stereo = AudioClip {
        sample_rate: 16_000,
        channels: 2,
        samples: [0.5, -0.5].repeat(16_000 * 3).into(),
    };
    let track = |name: &str, clip: &AudioClip| {
        RenderTrack::audio(name, Arc::new(Modulator::new(clip)), 0.0)
    };
    let graph = r#"{ "version": 0,
      "nodes": [
        { "id": "video", "type": "video_input" },
        { "id": "a", "type": "audio_input", "params": { "source": "low" } },
        { "id": "b", "type": "audio_input", "params": { "source": "high" } },
        { "id": "mix", "type": "combine" }, { "id": "sound", "type": "audio_output" },
        { "id": "out", "type": "output" }
      ],
      "connections": [
        { "from": "video", "to": "out" }, { "from": "a", "to": "mix.c1" }, { "from": "b", "to": "mix.c2" },
        { "from": "mix", "to": "sound" }
      ] }"#;
    let mut r = Renderer::new(
        &common::backend(),
        None,
        &common::tracks(vec![track("low", &mono), track("high", &stereo)]),
        &GraphDesc::from_json(graph).unwrap(),
        Default::default(),
        &rastersong_engine::Bus::main(),
        &Registry::default(),
        OutputSize::Native,
    )
    .unwrap();
    let stats = r.node_stats();
    let of = |id: &str| stats.iter().find(|s| &*s.node == id).unwrap();
    assert_eq!(of("a").outputs[0].len(), 300);
    assert_eq!(of("b").outputs[0].len(), 533 * 2);
    // The stretch is a note, not a problem.
    let mix = of("mix");
    assert_eq!(mix.diagnostics.len(), 1);
    assert_eq!(mix.diagnostics[0].severity, Severity::Note);
    assert!(matches!(
        r.audio_sink(),
        AudioSink::Rendered { channels: 2, .. }
    ));
    r.render(5, &|| false).unwrap();
    let block = r.audio().unwrap();
    // Left is the 9 kHz track; right is the stereo track squeezed in, its L and R alternating.
    let left: Vec<f32> = block.samples.iter().step_by(2).copied().collect();
    assert!(
        left[100..].iter().all(|&x| (x - 0.25).abs() < 1e-3),
        "{:?}",
        &left[100..110]
    );
}

#[test]
fn the_warmup_cap_limits_only_the_pre_render_never_the_effect() {
    let expected = sequential(INFINITE, OutputSize::Native);

    // Played from the start, the graph renders the same whatever the cap is.
    for cap in [0, 3, 120, 9999] {
        let mut r = renderer(INFINITE);
        r.set_max_warmup_frames(cap);
        assert!(r.warmup_frames() <= cap as usize);
        for (i, want) in expected.iter().enumerate() {
            let frame = r.render(i, &|| false).unwrap().unwrap();
            assert_eq!(frame, *want, "cap {cap}, frame {i}");
        }
    }

    // Only a seek is affected: a smaller cap pre-renders less, so it is further from exact.
    let seek_error = |cap: u32| {
        let mut r = renderer(INFINITE);
        r.set_max_warmup_frames(cap);
        let frame = r.render(40, &|| false).unwrap().unwrap();
        frame
            .iter()
            .zip(&expected[40])
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap()
    };
    assert!(seek_error(0) > seek_error(9999));
}

/// Renders the video track `video` through a passthrough graph on `timebase`, and returns the red
/// value (the fake video's frame index) and size of every frame.
fn passthrough_frames(
    timebase: Option<rastersong_engine::Timebase>,
    video: rastersong_engine::RenderTrack,
) -> (rastersong_engine::RenderInfo, Vec<u8>) {
    use rastersong_engine::{GraphDesc, Renderer};
    let mut r = Renderer::new(
        &common::backend(),
        timebase,
        &[video],
        &GraphDesc::from_json(common::FINITE_PASSTHROUGH).unwrap(),
        Default::default(),
        &rastersong_engine::Bus::main(),
        &Registry::default(),
        OutputSize::Native,
    )
    .unwrap();
    let info = *r.info();
    let reds = (0..info.frames)
        .map(|i| r.render(i, &|| false).unwrap().unwrap()[0])
        .collect();
    (info, reds)
}

#[test]
fn the_project_timebase_is_the_clock() {
    use rastersong_engine::{Rational, RenderTrack, Timebase};

    // Without a timebase of its own the project takes the first video's.
    let (info, reds) = passthrough_frames(None, RenderTrack::video("video", common::VIDEO));
    assert_eq!(
        (info.width, info.height, info.frames),
        (16, 8, common::FRAMES)
    );
    assert_eq!(info.frame_rate, Rational::new(30, 1));
    assert_eq!(reds, (0..common::FRAMES as u8).collect::<Vec<_>>());

    // At half the video's rate and size, every other frame is shown, scaled to the project.
    let timebase = Timebase {
        width: 8,
        height: 4,
        frame_rate: Rational::new(15, 1),
    };
    let (info, reds) =
        passthrough_frames(Some(timebase), RenderTrack::video("video", common::VIDEO));
    assert_eq!((info.width, info.height, info.frames), (8, 4, 30));
    assert_eq!(info.timebase, timebase);
    assert_eq!(reds, (0..30).map(|i| 2 * i as u8).collect::<Vec<_>>());
}

#[test]
fn items_place_the_video_and_gaps_read_zeros() {
    use rastersong_engine::{Item, RenderTrack};

    // Half a second in, from frame 10 of the video to frame 30, at double speed: 10 frames.
    let mut video = RenderTrack::video("video", common::VIDEO);
    video.items = vec![Item {
        position: 0.5,
        start: 10.0 / 30.0,
        end: Some(1.0),
        rate: 2.0,
        muted: false,
    }];
    let (info, reds) = passthrough_frames(None, video.clone());
    assert_eq!(info.frames, 25);
    assert!(reds[..15].iter().all(|&r| r == 0), "before the item: zeros");
    assert_eq!(
        reds[15..],
        (0..10).map(|i| 10 + 2 * i as u8).collect::<Vec<_>>()
    );

    // A muted item is a gap.
    video.items[0].muted = true;
    let (_, reds) = passthrough_frames(None, video);
    assert!(reds.iter().all(|&r| r == 0));
}

#[test]
fn a_video_input_naming_no_track_reads_zeros() {
    use rastersong_engine::RenderTrack;

    let (info, reds) = passthrough_frames(None, RenderTrack::video("other", common::VIDEO));
    assert_eq!(info.frames, common::FRAMES);
    assert!(reds.iter().all(|&r| r == 0));
}

#[test]
fn the_bypassed_graph_shows_the_top_video_track_playing() {
    use rastersong_engine::{GraphDesc, Item, Renderer, render_form};

    // Track `top` plays the first half second of the clip from 1 s; `bottom` the whole clip.
    let top = RenderTrack {
        items: vec![Item {
            end: Some(0.5),
            ..Item::whole(1.0)
        }],
        ..RenderTrack::video("top", common::VIDEO)
    };
    let bottom = RenderTrack::video("bottom", common::VIDEO);
    let graph = GraphDesc::from_json(common::FINITE).unwrap();
    let mut r = Renderer::new(
        &common::backend(),
        None,
        &[top, bottom],
        &render_form(&graph, &Registry::default(), true),
        Default::default(),
        &rastersong_engine::Bus::main(),
        &Registry::default(),
        OutputSize::Native,
    )
    .unwrap();
    let reds: Vec<u8> = (0..common::FRAMES)
        .map(|i| r.render(i, &|| false).unwrap().unwrap()[0])
        .collect();
    let expected: Vec<u8> = (0..common::FRAMES as u8)
        .map(|i| if (30..45).contains(&i) { i - 30 } else { i })
        .collect();
    assert_eq!(reds, expected);
}

#[test]
fn only_the_rendered_bus_has_its_audio_output_compiled() {
    use rastersong_engine::{AudioSink, Bus, GraphDesc, Renderer};

    let graph = GraphDesc::from_json(&SOUND.replace(
        r#"{ "id": "sound", "type": "audio_output" }"#,
        r#"{ "id": "sound", "type": "audio_output", "params": { "bus": "Surround" } }"#,
    ))
    .unwrap();
    let sink_for = |bus: &Bus| {
        let r = Renderer::new(
            &common::backend(),
            None,
            &common::tracks(vec![RenderTrack::audio(
                "audio",
                Arc::new(rastersong_engine::sources::Modulator::new(&common::audio())),
                0.0,
            )]),
            &graph,
            Default::default(),
            bus,
            &Registry::default(),
            OutputSize::Native,
        )
        .unwrap();
        r.audio_sink()
    };
    // Main has no Audio Output: it plays its track mix.
    assert_eq!(sink_for(&Bus::main()), AudioSink::TrackMix);
    // The Surround bus renders it with its six channels, mono going to every one.
    let surround = Bus {
        name: "Surround".into(),
        channels: 6,
    };
    assert_eq!(
        sink_for(&surround),
        AudioSink::Rendered {
            sample_rate: 48_000,
            channels: 6
        }
    );
}
