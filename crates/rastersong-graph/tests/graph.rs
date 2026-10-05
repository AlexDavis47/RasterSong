use std::collections::HashMap;

use rastersong_graph::{
    Category, CompileOptions, Graph, GraphDesc, GraphError, Layout, LayoutContext, Node, NodeSpec,
    ParamSpec, Params, ProcessContext, Registry, Signal,
};

const W: u32 = 4;
const H: u32 = 2;
const AUDIO: u32 = 5;

fn options() -> CompileOptions {
    CompileOptions {
        frame_rate: 30.0,
        tempo: Default::default(),
        sources: HashMap::from([
            ("video".to_owned(), Layout::rgb(W, H)),
            ("audio".to_owned(), Layout::audio(AUDIO)),
        ]),
        output: Layout::rgb(W, H),
    }
}

fn compile_with(json: &str, registry: &Registry) -> Result<Graph, GraphError> {
    Graph::compile(&GraphDesc::from_json(json)?, registry, &options())
}

fn compile(json: &str) -> Result<Graph, GraphError> {
    compile_with(json, &Registry::default())
}

/// Source signals for one frame: `video[i] = video(i)`, `audio[i] = audio(i)`.
fn sources(video: impl Fn(usize) -> f32, audio: impl Fn(usize) -> f32) -> HashMap<String, Signal> {
    let (v, a) = (Layout::rgb(W, H), Layout::audio(AUDIO));
    HashMap::from([
        (
            "video".to_owned(),
            Signal::from_data(v, (0..v.len()).map(video).collect()),
        ),
        (
            "audio".to_owned(),
            Signal::from_data(a, (0..a.len()).map(audio).collect()),
        ),
    ])
}

fn graph_json(nodes: &str, connections: &str) -> String {
    format!(r#"{{ "version": 1, "nodes": [{nodes}], "connections": [{connections}] }}"#)
}

const PASSTHROUGH: &str = r#"{ "version": 1,
    "nodes": [ { "id": "video", "type": "video_input" }, { "id": "out", "type": "output" } ],
    "connections": [ { "from": "video", "to": "out" } ] }"#;

#[test]
fn passthrough_returns_the_video() {
    let mut graph = compile(PASSTHROUGH).unwrap();
    let input = sources(|i| i as f32 / 100.0, |_| 0.0);
    let out = graph.process(0, &input).unwrap();
    assert_eq!(out.layout, Layout::rgb(W, H));
    assert_eq!(out.data, input["video"].data);
    assert_eq!(graph.latency_frames(), 0);
}

#[test]
fn split_and_combine_round_trip_with_swapped_channels() {
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "split", "type": "split" },
           { "id": "combine", "type": "combine" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "split" },
           { "from": "split.b", "to": "combine.r" }, { "from": "split.g", "to": "combine.g" },
           { "from": "split.r", "to": "combine.b" }, { "from": "combine", "to": "out" }"#,
    );
    let mut graph = compile(&json).unwrap();
    let input = sources(|i| i as f32, |_| 0.0);
    let out = graph.process(0, &input).unwrap();
    for (o, i) in out.data.chunks(3).zip(input["video"].data.chunks(3)) {
        assert_eq!(o, [i[2], i[1], i[0]]);
    }
}

#[test]
fn interleave_and_pack_round_trip() {
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "i", "type": "interleave" },
           { "id": "p", "type": "pack" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "i" }, { "from": "i", "to": "p" }, { "from": "p", "to": "out" }"#,
    );
    let mut graph = compile(&json).unwrap();
    let input = sources(|i| i as f32, |_| 0.0);
    assert_eq!(graph.process(0, &input).unwrap().data, input["video"].data);
}

#[test]
fn modulators_are_resampled_to_the_carrier() {
    // AM with depth 1 on a carrier of 1.0: out = 1 + modulator, so the output shows exactly how
    // the 5 audio samples were spread over 8 pixels (held, R/G/B of a pixel kept together).
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
           { "id": "am", "type": "am" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "am.carrier" }, { "from": "audio", "to": "am.modulator" },
           { "from": "am", "to": "out" }"#,
    );
    let mut graph = compile(&json).unwrap();
    let input = sources(|_| 1.0, |i| i as f32);
    let out = graph.process(0, &input).unwrap();
    let pixels: Vec<f32> = out
        .data
        .chunks(3)
        .map(|p| {
            assert!(p[0] == p[1] && p[1] == p[2], "pixel channels differ: {p:?}");
            p[0] - 1.0
        })
        .collect();
    // Pixel centres at (p + 0.5) * 5/8 → source samples 0, 0, 1, 2, 2, 3, 4, 4.
    assert_eq!(pixels, [0.0, 0.0, 1.0, 2.0, 2.0, 3.0, 4.0, 4.0]);
}

#[test]
fn unused_nodes_are_not_run() {
    // A node reading a source the host doesn't provide is fine as long as it doesn't feed the output.
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "other", "type": "video_input", "params": { "source": "nope" } },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "out" }"#,
    );
    assert!(compile(&json).is_ok());
}

#[test]
fn unconnected_inputs_read_zeros() {
    // The output with nothing connected renders black.
    let mut graph = compile(&graph_json(r#"{ "id": "out", "type": "output" }"#, "")).unwrap();
    let out = graph.process(0, &sources(|_| 0.5, |_| 0.5)).unwrap();
    assert!(out.data.iter().all(|&x| x == 0.0));

    // So does a node on the way to it: the delay's main input is empty, and `am`'s modulator too.
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "am", "type": "am" },
           { "id": "d", "type": "delay" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "am.carrier" }, { "from": "am", "to": "out" }"#,
    );
    let mut graph = compile(&json).unwrap();
    let out = graph.process(0, &sources(|_| 0.5, |_| 0.5)).unwrap();
    assert!(out.data.iter().all(|x| x.is_finite()));
}

fn render_form_of(json: &str, bypass_all: bool) -> GraphDesc {
    rastersong_graph::render_form(
        &GraphDesc::from_json(json).unwrap(),
        &Registry::default(),
        bypass_all,
    )
}

const CHAIN: &str = r#"{ "version": 2,
    "nodes": [
        { "id": "video", "type": "video_input" },
        { "id": "d", "type": "delay", "params": { "time": 1 } },
        { "id": "out", "type": "output" }
    ],
    "connections": [ { "from": "video", "to": "d" }, { "from": "d", "to": "out" } ] }"#;

#[test]
fn render_form_ignores_nodes_that_do_not_feed_the_output() {
    let base = render_form_of(CHAIN, false);
    // An orphan node, an orphan wired from the chain, and editor-only details change nothing.
    let with_orphans = CHAIN
        .replace(
            r#"{ "id": "out", "type": "output" }"#,
            r#"{ "id": "out", "type": "output", "position": [4, 5] },
           { "id": "x", "type": "bitcrush", "params": { "bits": 3 } },
           { "id": "n", "type": "noise", "label": "grain" }"#,
        )
        .replace(
            r#"{ "from": "d", "to": "out" }"#,
            r#"{ "from": "d", "to": "out" }, { "from": "d", "to": "x" }, { "from": "n", "to": "x.@bits" }"#,
        );
    assert_eq!(render_form_of(&with_orphans, false), base);
    // Changing a node that does feed the output is a change.
    let louder = CHAIN.replace(r#""time": 1"#, r#""time": 2"#);
    assert_ne!(render_form_of(&louder, false), base);
    // So is bypassing it, but bypassing an orphan is not.
    let bypassed = CHAIN.replace(
        r#""id": "d", "type": "delay""#,
        r#""id": "d", "type": "delay", "bypass": true"#,
    );
    assert_ne!(render_form_of(&bypassed, false), base);
    let orphan_bypassed = with_orphans.replace(
        r#""id": "x", "type": "bitcrush""#,
        r#""id": "x", "type": "bitcrush", "bypass": true"#,
    );
    assert_eq!(render_form_of(&orphan_bypassed, false), base);
}

#[test]
fn render_form_with_global_bypass_wires_the_video_to_the_output() {
    let form = render_form_of(CHAIN, true);
    let ids: Vec<&str> = form.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["video", "out"]);
    let mut graph = Graph::compile(&form, &Registry::default(), &options()).unwrap();
    let out = graph
        .process(0, &sources(|i| i as f32 / 10.0, |_| 0.0))
        .unwrap();
    assert_eq!(out.data[3], 0.3);
}

#[test]
fn reports_graph_errors() {
    type Check = fn(&GraphError) -> bool;
    let cases: &[(&str, &str, Check)] = &[
        (r#"{ "id": "x", "type": "nope" }"#, "", |e| {
            matches!(e, GraphError::UnknownNodeType { .. })
        }),
        (
            r#"{ "id": "o", "type": "output" }, { "id": "o", "type": "output" }"#,
            "",
            |e| matches!(e, GraphError::DuplicateId(_)),
        ),
        (r#"{ "id": "v", "type": "video_input" }"#, "", |e| {
            matches!(e, GraphError::OutputCount(0))
        }),
        (
            r#"{ "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
            r#"{ "from": "v.nope", "to": "o" }"#,
            |e| matches!(e, GraphError::Connection { .. }),
        ),
        (
            r#"{ "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
            r#"{ "from": "v", "to": "o" }, { "from": "v", "to": "o" }"#,
            |e| matches!(e, GraphError::Connection { .. }),
        ),
        (
            r#"{ "id": "v", "type": "video_input" }, { "id": "d", "type": "delay", "params": { "tme": 1 } }, { "id": "o", "type": "output" }"#,
            r#"{ "from": "v", "to": "d" }, { "from": "d", "to": "o" }"#,
            |e| matches!(e, GraphError::Node { message, .. } if message.contains("tme")),
        ),
        (
            r#"{ "id": "a", "type": "am" }, { "id": "b", "type": "am" }, { "id": "v", "type": "video_input" }, { "id": "o", "type": "output" }"#,
            r#"{ "from": "a", "to": "b.carrier" }, { "from": "b", "to": "a.carrier" }, { "from": "v", "to": "a.modulator" },
               { "from": "v", "to": "b.modulator" }, { "from": "a", "to": "o" }"#,
            |e| matches!(e, GraphError::Cycle(_)),
        ),
        (
            // Combine expects mono inputs.
            r#"{ "id": "v", "type": "video_input" }, { "id": "c", "type": "combine" }, { "id": "o", "type": "output" }"#,
            r#"{ "from": "v", "to": "c.r" }, { "from": "v", "to": "c.g" }, { "from": "v", "to": "c.b" }, { "from": "c", "to": "o" }"#,
            |e| matches!(e, GraphError::Node { node, .. } if node == "c"),
        ),
        (
            // Output must match the project size.
            r#"{ "id": "a", "type": "audio_input" }, { "id": "o", "type": "output" }"#,
            r#"{ "from": "a", "to": "o" }"#,
            |e| matches!(e, GraphError::Node { node, .. } if node == "o"),
        ),
    ];
    for (nodes, connections, check) in cases {
        let err = compile(&graph_json(nodes, connections)).unwrap_err();
        assert!(check(&err), "unexpected error for {nodes}: {err}");
    }
}

#[test]
fn missing_sources_are_an_error_at_process_time() {
    let mut graph = compile(PASSTHROUGH).unwrap();
    let err = graph.process(0, &HashMap::new()).unwrap_err();
    assert!(matches!(err, GraphError::Source { .. }));
}

#[test]
fn latency_is_compensated_across_branches() {
    // Half a frame of lookahead on one branch only. Without compensation, `sum` would add two
    // signals half a frame apart. The output is rounded up to a whole frame of latency.
    let frame = Layout::rgb(W, H).len();
    let mut registry = Registry::default();
    rastersong_graph::testing::register_fakes(&mut registry);
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "look", "type": "lookahead" },
           { "id": "sum", "type": "sum" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "look" }, { "from": "look", "to": "sum.a" },
           { "from": "video", "to": "sum.b" }, { "from": "sum", "to": "out" }"#,
    );
    let mut graph = compile_with(&json, &registry).unwrap();
    assert_eq!(graph.latency_frames(), 1);

    // Sample i of frame n has value n * frame + i, a continuous ramp.
    let mut outputs = Vec::new();
    for n in 0..4 {
        let input = sources(move |i| (n * frame + i) as f32, |_| 0.0);
        outputs.push(graph.process(n as u64, &input).unwrap().data.clone());
    }
    // Frame n of output is source frame n - 1, doubled.
    for (n, output) in outputs.iter().enumerate().skip(1) {
        let expected: Vec<f32> = (0..frame)
            .map(|i| 2.0 * ((n - 1) * frame + i) as f32)
            .collect();
        assert_eq!(*output, expected, "frame {n}");
    }
}

#[test]
fn reset_reproduces_output() {
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
           { "id": "lp", "type": "lowpass", "params": { "cutoff": 2, "depth": 1 } },
           { "id": "d", "type": "delay", "params": { "time": 0.5, "depth": 0.25, "feedback": 0.5 } },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "lp" }, { "from": "audio", "to": "lp.modulation" },
           { "from": "lp", "to": "d" }, { "from": "audio", "to": "d.modulation" }, { "from": "d", "to": "out" }"#,
    );
    let mut graph = compile(&json).unwrap();
    let run = |graph: &mut Graph| -> Vec<Vec<f32>> {
        (0..5)
            .map(|n| {
                let input = sources(
                    move |i| ((i * 7 + n * 3) % 11) as f32 / 10.0,
                    move |i| ((i + n) as f32).sin(),
                );
                graph.process(n as u64, &input).unwrap().data.clone()
            })
            .collect()
    };
    let first = run(&mut graph);
    graph.reset();
    assert_eq!(run(&mut graph), first);
}

#[test]
fn graph_files_round_trip() {
    let desc = GraphDesc::from_json(PASSTHROUGH).unwrap();
    assert_eq!(GraphDesc::from_json(&desc.to_json()).unwrap(), desc);
    assert!(matches!(
        GraphDesc::from_json(r#"{ "version": 3, "nodes": [] }"#),
        Err(GraphError::Parse(_))
    ));
}

#[test]
fn separate_channels_match_splitting_by_hand() {
    // A low pass with "channels": "separate" must equal Split -> three low passes -> Combine,
    // including the modulation input, which is shared by all three channels.
    let separate = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
           { "id": "lp", "type": "lowpass", "params": { "cutoff": 0.7, "depth": 1 }, "channels": "separate" },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "lp" }, { "from": "audio", "to": "lp.modulation" }, { "from": "lp", "to": "out" }"#,
    );
    let by_hand = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
           { "id": "split", "type": "split" }, { "id": "combine", "type": "combine" },
           { "id": "r", "type": "lowpass", "params": { "cutoff": 0.7, "depth": 1 } },
           { "id": "g", "type": "lowpass", "params": { "cutoff": 0.7, "depth": 1 } },
           { "id": "b", "type": "lowpass", "params": { "cutoff": 0.7, "depth": 1 } },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "split" },
           { "from": "split.r", "to": "r" }, { "from": "split.g", "to": "g" }, { "from": "split.b", "to": "b" },
           { "from": "audio", "to": "r.modulation" }, { "from": "audio", "to": "g.modulation" }, { "from": "audio", "to": "b.modulation" },
           { "from": "r", "to": "combine.r" }, { "from": "g", "to": "combine.g" }, { "from": "b", "to": "combine.b" },
           { "from": "combine", "to": "out" }"#,
    );
    let together = separate.replace(r#", "channels": "separate""#, "");

    let (mut a, mut b, mut c) = (
        compile(&separate).unwrap(),
        compile(&by_hand).unwrap(),
        compile(&together).unwrap(),
    );
    let mut differs_from_together = false;
    for n in 0..4 {
        let input = sources(
            move |i| ((i * 7 + n * 5) % 13) as f32 / 12.0,
            move |i| ((i + n) as f32 * 0.9).sin(),
        );
        let sep = a.process(n as u64, &input).unwrap().data.clone();
        assert_eq!(sep, b.process(n as u64, &input).unwrap().data, "frame {n}");
        differs_from_together |= sep != c.process(n as u64, &input).unwrap().data;
    }
    assert!(
        differs_from_together,
        "together and separate should differ for a filter"
    );
}

#[test]
fn separate_channels_need_a_per_channel_node() {
    let json = graph_json(
        r#"{ "id": "v", "type": "video_input" }, { "id": "s", "type": "interleave", "channels": "separate" },
           { "id": "p", "type": "pack" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "s" }, { "from": "s", "to": "p" }, { "from": "p", "to": "o" }"#,
    );
    assert!(matches!(compile(&json), Err(GraphError::Node { node, .. }) if node == "s"));
}

#[test]
fn reports_output_levels() {
    let mut graph = compile(PASSTHROUGH).unwrap();
    let input = sources(|_| 0.5, |_| 0.0);
    graph.process(0, &input).unwrap();
    let levels = graph.levels();
    let video = levels.iter().find(|l| &*l.node == "video").unwrap();
    assert!((video.rms - 0.5).abs() < 1e-6);
    assert_eq!(levels.len(), 2, "one output each for video and out");
}

/// AM on a carrier and modulator of 1.0 gives `1 + depth`, so the output shows the per-sample
/// depth. The audio (5 samples, `a[i] = i / 10`) modulates the depth.
fn am_with_modulated_depth(modulation: &str, channels: &str) -> Vec<f32> {
    let json = graph_json(
        &format!(
            r#"{{ "id": "video", "type": "video_input" }}, {{ "id": "audio", "type": "audio_input" }},
               {{ "id": "am", "type": "am", "channels": "{channels}", "modulation": {{ "depth": {modulation} }} }},
               {{ "id": "out", "type": "output" }}"#
        ),
        r#"{ "from": "video", "to": "am.carrier" }, { "from": "video", "to": "am.modulator" },
           { "from": "audio", "to": "am.@depth" }, { "from": "am", "to": "out" }"#,
    );
    let mut graph = compile(&json).unwrap();
    let input = sources(|_| 1.0, |i| i as f32 / 10.0);
    graph.process(0, &input).unwrap().data.clone()
}

/// The audio sample each of the 8 pixels holds when 5 samples are stretched over them.
fn held_audio(pixel: usize) -> f32 {
    let i = ((pixel as f64 + 0.5) * f64::from(AUDIO) / f64::from(W * H)) as usize;
    i as f32 / 10.0
}

#[test]
fn a_signal_connected_to_a_parameter_modulates_it_per_sample() {
    // Bipolar: depth = 1 + 0.5 × a.
    let out = am_with_modulated_depth(r#"{ "amount": 0.5 }"#, "together");
    for (pixel, rgb) in out.chunks(3).enumerate() {
        let expected = 1.0 + (1.0 + 0.5 * held_audio(pixel));
        assert!(
            rgb.iter().all(|&x| (x - expected).abs() < 1e-6),
            "pixel {pixel}: {rgb:?}"
        );
    }
    // Unipolar with a negative amount turns it down by the magnitude: depth = 1 - 2 × |a|.
    let out = am_with_modulated_depth(r#"{ "amount": -2, "mode": "unipolar" }"#, "together");
    for (pixel, rgb) in out.chunks(3).enumerate() {
        let expected = 1.0 + (1.0 - 2.0 * held_audio(pixel));
        assert!((rgb[0] - expected).abs() < 1e-6, "pixel {pixel}: {rgb:?}");
    }
}

#[test]
fn modulated_parameters_work_with_separate_channels() {
    // AM is per-sample, so processing channels separately must give the same result.
    let together = am_with_modulated_depth(r#"{ "amount": 0.5 }"#, "together");
    let separate = am_with_modulated_depth(r#"{ "amount": 0.5 }"#, "separate");
    assert_eq!(together, separate);
}

#[test]
fn modulated_values_stay_within_the_parameter_limits() {
    // Bit crush depth is limited to ±1000; a huge amount is clamped rather than passed on.
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
           { "id": "crush", "type": "bitcrush", "modulation": { "bits": { "amount": 1000 } } },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "crush" }, { "from": "audio", "to": "crush.@bits" },
           { "from": "crush", "to": "out" }"#,
    );
    let mut graph = compile(&json).unwrap();
    // Bits are limited to 1..24 inside the node; the output is still finite and in range.
    let out = graph
        .process(0, &sources(|i| i as f32 / 30.0, |i| i as f32))
        .unwrap();
    assert!(
        out.data
            .iter()
            .all(|x| x.is_finite() && (0.0..=1.0).contains(x))
    );
}

#[test]
fn parameter_connections_are_checked() {
    let graph = |target: &str, modulation: &str| {
        graph_json(
            &format!(
                r#"{{ "id": "video", "type": "video_input" }}, {{ "id": "audio", "type": "audio_input" }},
                   {{ "id": "bands", "type": "three_band" }}, {{ "id": "am", "type": "am", "modulation": {modulation} }},
                   {{ "id": "out", "type": "output" }}"#
            ),
            &format!(
                r#"{{ "from": "video", "to": "am.carrier" }}, {{ "from": "audio", "to": "bands" }},
                   {{ "from": "bands", "to": "am.modulator" }}, {{ "from": "audio", "to": "{target}" }},
                   {{ "from": "am", "to": "out" }}"#
            ),
        )
    };
    let error = |json: String| compile(&json).unwrap_err().to_string();
    assert!(error(graph("am.@nope", "{}")).contains("no parameter `nope`"));
    assert!(error(graph("bands.@low_hz", "{}")).contains("no parameter `low_hz`"));
    assert!(error(graph("am.@depth", r#"{ "nope": { "amount": 1 } }"#)).contains("`nope`"));
    assert!(compile(&graph("am.@depth", "{}")).is_ok());
}

/// Test node: an input-less generator. It takes its layout from the host's video (without being
/// a source node itself) and outputs its `level` parameter in every sample.
struct Level {
    level: f32,
}

impl Node for Level {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        ctx.sources
            .get("video")
            .map(|&layout| vec![layout])
            .ok_or_else(|| "needs the video's layout".to_owned())
    }

    fn process(&mut self, ctx: &ProcessContext, _: &[&Signal], outputs: &mut [Signal]) {
        let level = ctx.value(0, f64::from(self.level));
        for (i, out) in outputs[0].data.iter_mut().enumerate() {
            *out = level.at(i);
        }
    }
}

const LEVEL_PARAMS: &[ParamSpec] = &[ParamSpec::number(
    "level",
    "Level",
    0.25,
    0.0,
    1.0,
    "The value to output",
)];

const LEVEL_SPEC: NodeSpec = NodeSpec::new("Level", Category::Input)
    .describe("Test generator")
    .params(LEVEL_PARAMS)
    .inputs(&[]);

fn level_registry() -> Registry {
    let mut registry = Registry::default();
    registry.register_custom("level", LEVEL_SPEC, |params: &Params| {
        Ok(Level {
            level: params.number("level")? as f32,
        })
    });
    registry
}

#[test]
fn nodes_without_inputs_get_their_layout_from_the_host_and_can_be_modulated() {
    let registry = level_registry();
    let plain = graph_json(
        r#"{ "id": "gen", "type": "level" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "gen", "to": "out" }"#,
    );
    let mut graph = compile_with(&plain, &registry).unwrap();
    let out = graph.process(0, &sources(|_| 0.0, |_| 0.0)).unwrap();
    assert_eq!(out.layout, Layout::rgb(W, H));
    assert!(out.data.iter().all(|&x| x == 0.25));

    // A signal connected to the parameter reaches a node that has no main input to measure it by.
    let modulated = graph_json(
        r#"{ "id": "gen", "type": "level", "modulation": { "level": { "amount": 1 } } },
           { "id": "audio", "type": "audio_input" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "audio", "to": "gen.@level" }, { "from": "gen", "to": "out" }"#,
    );
    let mut graph = compile_with(&modulated, &registry).unwrap();
    let out = graph.process(0, &sources(|_| 0.0, |_| 0.5)).unwrap();
    assert!(
        out.data.iter().all(|&x| (x - 0.75).abs() < 1e-6),
        "{:?}",
        &out.data[..4]
    );
}

#[test]
fn generators_take_their_layout_from_the_host_and_draw_stripes() {
    // A ramp of one cycle per row on the 4×2 RGB video: each pixel's channels share a value.
    let json = graph_json(
        r#"{ "id": "ramp", "type": "oscillator", "params": { "wave": "ramp", "freq": 1 } },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "ramp", "to": "out" }"#,
    );
    let mut graph = compile(&json).unwrap();
    let out = graph.process(0, &sources(|_| 0.0, |_| 0.0)).unwrap();
    assert_eq!(out.layout, Layout::rgb(W, H));
    let row: Vec<f32> = [0.0, 0.25, 0.5, 0.75]
        .into_iter()
        .flat_map(|v| [v; 3])
        .collect();
    assert_eq!(out.data, [row.clone(), row].concat());
}

#[test]
fn the_project_tempo_reaches_beat_nodes() {
    // A pulse on the first half of every beat, on 24-sample frames at 30 fps (720 samples a
    // second). At 400 bpm a beat is 108 samples (4.5 frames); at 100 bpm it is 432 (18 frames).
    let json = graph_json(
        r#"{ "id": "beat", "type": "beat", "params": { "shape": "pulse", "width": 0.5 } },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "beat", "to": "out" }"#,
    );
    let first_sample = |bpm: f64, frame: u64| {
        let mut options = options();
        options.tempo = rastersong_graph::Tempo {
            bpm,
            ..Default::default()
        };
        let mut graph = Graph::compile(
            &GraphDesc::from_json(&json).unwrap(),
            &Registry::default(),
            &options,
        )
        .unwrap();
        graph
            .process(frame, &sources(|_| 0.0, |_| 0.0))
            .unwrap()
            .data[0]
    };
    // Frame 3 starts 72 samples (24 pixels) in: past half of a 400 bpm beat, early in a 100 bpm one.
    assert_eq!(first_sample(400.0, 3), 0.0);
    assert_eq!(first_sample(100.0, 3), 1.0);
}

#[test]
fn bypassed_nodes_pass_their_main_input_through() {
    let json = |bypass: &str| {
        graph_json(
            &format!(
                r#"{{ "id": "video", "type": "video_input" }},
                {{ "id": "a", "type": "delay" {bypass} }}, {{ "id": "b", "type": "delay" {bypass} }},
                {{ "id": "out", "type": "output" }}"#
            ),
            r#"{ "from": "video", "to": "a" }, { "from": "a", "to": "b" },
               { "from": "b", "to": "out" }"#,
        )
    };
    let input = sources(|i| i as f32 / 100.0, |_| 0.0);
    let mut bypassed = compile(&json(r#", "bypass": true"#)).unwrap();
    assert_eq!(
        bypassed.process(0, &input).unwrap().data,
        input["video"].data
    );
    assert_eq!(bypassed.latency_frames(), 0);
    // Without the flag the same chain is a real node chain, and the flag round-trips.
    let plain = GraphDesc::from_json(&json("")).unwrap();
    assert!(plain.nodes.iter().all(|n| !n.bypass));
    let flagged = GraphDesc::from_json(&json(r#", "bypass": true"#)).unwrap();
    assert!(flagged.to_json().contains(r#""bypass": true"#));
}
