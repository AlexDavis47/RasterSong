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
    assert!(out.layout.same_shape(&Layout::rgb(W, H)));
    assert_eq!(out.data, input["video"].data);
    assert_eq!(graph.latency_frames(), 0);
}

#[test]
fn split_and_combine_round_trip_with_swapped_channels() {
    // Written with the old RGB port names, which load as numbered channels.
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
            // Pack needs rows that divide into whole pixels: 12 samples into pixels of 5.
            r#"{ "id": "v", "type": "video_input" }, { "id": "p", "type": "pack", "params": { "channels": 5 } }, { "id": "o", "type": "output" }"#,
            r#"{ "from": "v", "to": "p" }, { "from": "p", "to": "o" }"#,
            |e| matches!(e, GraphError::Node { node, .. } if node == "p"),
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
        GraphDesc::from_json(r#"{ "version": 7, "nodes": [] }"#),
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
           { "from": "split.c1", "to": "r" }, { "from": "split.c2", "to": "g" }, { "from": "split.c3", "to": "b" },
           { "from": "audio", "to": "r.modulation" }, { "from": "audio", "to": "g.modulation" }, { "from": "audio", "to": "b.modulation" },
           { "from": "r", "to": "combine.c1" }, { "from": "g", "to": "combine.c2" }, { "from": "b", "to": "combine.c3" },
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
fn separate_channels_on_a_node_that_cant_fall_back_to_together_with_a_warning() {
    let json = graph_json(
        r#"{ "id": "v", "type": "video_input" }, { "id": "s", "type": "interleave", "channels": "separate" },
           { "id": "p", "type": "pack" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "v", "to": "s" }, { "from": "s", "to": "p" }, { "from": "p", "to": "o" }"#,
    );
    let mut graph = compile(&json).unwrap();
    let warnings = graph.diagnostics();
    assert!(
        warnings
            .iter()
            .any(|d| &*d.node == "s" && d.message.contains("together")),
        "{warnings:?}"
    );
    // The setting can't be honoured, so this one is a warning.
    assert!(
        warnings
            .iter()
            .any(|d| d.severity == rastersong_graph::Severity::Warning)
    );
    let input = sources(|i| i as f32, |_| 0.0);
    assert_eq!(graph.process(0, &input).unwrap().data, input["video"].data);
}

#[test]
fn separate_channels_on_a_mono_signal_is_a_warning() {
    let json = graph_json(
        r#"{ "id": "a", "type": "audio_input" }, { "id": "d", "type": "delay", "channels": "separate" },
           { "id": "v", "type": "video_input" }, { "id": "am", "type": "am" }, { "id": "o", "type": "output" }"#,
        r#"{ "from": "a", "to": "d" }, { "from": "v", "to": "am.carrier" }, { "from": "d", "to": "am.modulator" },
           { "from": "am", "to": "o" }"#,
    );
    let graph = compile(&json).unwrap();
    assert!(graph.diagnostics().iter().any(|d| &*d.node == "d"));
}

/// The compiled tag of `node`'s output `port`.
fn tag_of(graph: &Graph, node: &str, port: usize) -> rastersong_graph::Tag {
    graph
        .node_stats()
        .iter()
        .find(|s| &*s.node == node)
        .unwrap_or_else(|| panic!("no node `{node}`"))
        .outputs[port]
        .tag
}

#[test]
fn tags_follow_the_signal_through_the_graph() {
    use rastersong_graph::{ChannelMap, Kind, Part, Range};
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "split", "type": "split" },
           { "id": "d", "type": "delay" }, { "id": "audio", "type": "to_audio" },
           { "id": "combine", "type": "combine" }, { "id": "back", "type": "to_video" },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "split" }, { "from": "split.c2", "to": "d" },
           { "from": "d", "to": "audio" }, { "from": "audio", "to": "combine.c1" },
           { "from": "split.c1", "to": "combine.c2" }, { "from": "split.c3", "to": "combine.c3" },
           { "from": "combine", "to": "back" }, { "from": "back", "to": "out" }"#,
    );
    let graph = compile(&json).unwrap();
    let video = tag_of(&graph, "video", 0);
    assert_eq!(
        (video.kind, video.channels, video.range),
        (Kind::Video, ChannelMap::Rgb, Range::Unipolar)
    );
    // A delay on the green channel is still green video.
    let green = tag_of(&graph, "d", 0);
    assert_eq!(
        (green.kind, green.part, green.channels),
        (Kind::Video, Part::Green, ChannelMap::Mono)
    );
    // Converted to audio: the part stays, the kind and range change.
    let audio = tag_of(&graph, "audio", 0);
    assert_eq!(
        (audio.kind, audio.part, audio.range),
        (Kind::Audio, Part::Green, Range::Bipolar)
    );
    // Three audio channels combined aren't RGB.
    let combined = tag_of(&graph, "combine", 0);
    assert_eq!(
        (combined.kind, combined.channels, combined.part),
        (Kind::Audio, ChannelMap::Numbered, Part::Whole)
    );
    assert_eq!(tag_of(&graph, "back", 0).channels, ChannelMap::Rgb);
}

#[test]
fn range_mismatches_are_warnings_not_errors() {
    // A gate expects audio's -1 to 1; video's 0 to 1 still renders, with a warning.
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "g", "type": "gate" },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "g" }, { "from": "g", "to": "out" }"#,
    );
    let mut graph = compile(&json).unwrap();
    let warnings = graph.diagnostics();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(&*warnings[0].node, "g");
    assert!(
        warnings[0].message.contains("-1 to 1"),
        "{}",
        warnings[0].message
    );
    // A suggestion, not a mistake: often it's the effect being made.
    assert_eq!(warnings[0].severity, rastersong_graph::Severity::Note);
    assert!(graph.process(0, &sources(|_| 0.5, |_| 0.0)).is_ok());
    // Audio into the output is shown, with a warning on the output.
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "a", "type": "to_audio" },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "a" }, { "from": "a", "to": "out" }"#,
    );
    let graph = compile(&json).unwrap();
    assert!(graph.diagnostics().iter().any(|d| &*d.node == "out"));
    // A clean graph has none.
    assert!(compile(PASSTHROUGH).unwrap().diagnostics().is_empty());
}

/// Options with stereo audio of `frames` frames.
fn stereo_options(frames: u32) -> CompileOptions {
    let mut options = options();
    options
        .sources
        .insert("audio".into(), Layout::audio_channels(frames, 2));
    options
}

#[test]
fn stereo_splits_into_left_and_right_and_combines_back() {
    use rastersong_graph::Part;
    // Swap left and right through Split and Combine, with 20 dB (ten times) on the left only.
    let json = graph_json(
        r#"{ "id": "audio", "type": "audio_input" }, { "id": "split", "type": "split" },
           { "id": "gain", "type": "gain", "params": { "gain": 20 } }, { "id": "combine", "type": "combine" },
           { "id": "v", "type": "video_input" }, { "id": "am", "type": "am" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "audio", "to": "split" }, { "from": "split.c1", "to": "gain" },
           { "from": "split.c2", "to": "combine.c1" }, { "from": "gain", "to": "combine.c2" },
           { "from": "v", "to": "am.carrier" }, { "from": "combine", "to": "am.modulator" }, { "from": "am", "to": "out" }"#,
    );
    let mut graph = Graph::compile(
        &GraphDesc::from_json(&json).unwrap(),
        &Registry::default(),
        &stereo_options(3),
    )
    .unwrap();
    assert_eq!(tag_of(&graph, "split", 0).part, Part::Left);
    assert_eq!(tag_of(&graph, "split", 1).part, Part::Right);
    let combined = graph
        .node_stats()
        .iter()
        .find(|s| &*s.node == "combine")
        .unwrap()
        .outputs[0];
    assert!(combined.same_shape(&Layout::audio_channels(3, 2)));
    let audio = Signal::from_data(
        Layout::audio_channels(3, 2),
        vec![1., -1., 2., -2., 3., -3.],
    );
    let mut input = sources(|_| 0.0, |_| 0.0);
    input.insert("audio".into(), audio);
    graph.process(0, &input).unwrap();
    // The combined signal's level: right (-1, -2, -3) then left ten times (10, 20, 30).
    let level = graph
        .levels()
        .into_iter()
        .find(|l| &*l.node == "combine")
        .unwrap()
        .rms;
    let expected = ((14.0 + 1400.0) / 6.0f32).sqrt();
    assert!((level - expected).abs() < 1e-5, "{level} vs {expected}");
}

#[test]
fn separate_channels_work_on_stereo() {
    // A one-sample delay per channel delays each of L and R by one frame of the pair.
    let json = |channels: &str| {
        graph_json(
            &format!(
                r#"{{ "id": "audio", "type": "audio_input" }},
                   {{ "id": "d", "type": "delay", "params": {{ "time": 1, "unit": "rows" }}, "channels": "{channels}" }},
                   {{ "id": "v", "type": "video_input" }}, {{ "id": "am", "type": "am" }}, {{ "id": "out", "type": "output" }}"#
            ),
            r#"{ "from": "audio", "to": "d" }, { "from": "v", "to": "am.carrier" },
               { "from": "d", "to": "am.modulator" }, { "from": "am", "to": "out" }"#,
        )
    };
    let graph = Graph::compile(
        &GraphDesc::from_json(&json("separate")).unwrap(),
        &Registry::default(),
        &stereo_options(4),
    )
    .unwrap();
    assert!(graph.diagnostics().is_empty(), "{:?}", graph.diagnostics());
    // One row of a stereo block is its whole length, so either way it's a one-frame delay.
    let stats = graph.node_stats().iter().find(|s| &*s.node == "d").unwrap();
    assert!(stats.outputs[0].same_shape(&Layout::audio_channels(4, 2)));
}

#[test]
fn grouping_chooses_how_mono_spreads_over_channels() {
    // 5 audio samples over 8 RGB pixels: grouped by pixel, a pixel's channels match; spread over
    // samples, they differ.
    let json = |grouping: &str| {
        graph_json(
            &format!(
                r#"{{ "id": "video", "type": "video_input" }}, {{ "id": "audio", "type": "audio_input" }},
                   {{ "id": "s", "type": "stretch", "grouping": "{grouping}" }}, {{ "id": "out", "type": "output" }}"#
            ),
            r#"{ "from": "video", "to": "s.like" }, { "from": "audio", "to": "s.in" }, { "from": "s", "to": "out" }"#,
        )
    };
    let input = sources(|_| 0.0, |i| i as f32);
    let mut pixels = compile(&json("pixels")).unwrap();
    let out = pixels.process(0, &input).unwrap().data.clone();
    assert!(
        out.chunks(3).all(|p| p[0] == p[1] && p[1] == p[2]),
        "{out:?}"
    );
    assert_eq!(out[..6], [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let mut samples = compile(&json("samples")).unwrap();
    let out = samples.process(0, &input).unwrap().data.clone();
    assert!(out.chunks(3).any(|p| p[0] != p[2]), "{out:?}");
    // The stretched signal is still audio, at the picture's size: the output warns about it.
    let graph = compile(&json("pixels")).unwrap();
    assert_eq!(tag_of(&graph, "s", 0).kind, rastersong_graph::Kind::Audio);
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
    assert!(out.layout.same_shape(&Layout::rgb(W, H)));
    assert!(out.data.iter().all(|&x| x == 0.25));

    // A signal connected to the parameter reaches a node that has no main input to measure it by.
    // (Custom node types aren't known to the upgrade, so this graph is written in the current
    // format: 100% one way moves the level across its whole 0..1 span.)
    let modulated = r#"{ "version": 6, "nodes": [
        { "id": "gen", "type": "level", "modulation": { "level": { "amount": 100, "mode": "unipolar" } } },
        { "id": "audio", "type": "audio_input" }, { "id": "out", "type": "output" } ],
        "connections": [ { "from": "audio", "to": "gen.@level" }, { "from": "gen", "to": "out" } ] }"#;
    let mut graph = compile_with(modulated, &registry).unwrap();
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
    assert!(out.layout.same_shape(&Layout::rgb(W, H)));
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

/// AM of a carrier and modulator of 1.0 with `depth` set to 1.6 and an optional modulation, with
/// `integer` listing the rounded parameters. The output is `1 + depth`.
fn am_with_integer_depth(extra: &str, wire_depth: bool) -> Vec<f32> {
    let depth_wire = if wire_depth {
        r#", { "from": "audio", "to": "am.@depth" }"#
    } else {
        ""
    };
    let json = graph_json(
        &format!(
            r#"{{ "id": "video", "type": "video_input" }}, {{ "id": "audio", "type": "audio_input" }},
               {{ "id": "am", "type": "am", "params": {{ "depth": 1.6 }}, {extra} }},
               {{ "id": "out", "type": "output" }}"#
        ),
        &format!(
            r#"{{ "from": "video", "to": "am.carrier" }}, {{ "from": "video", "to": "am.modulator" }},
               {{ "from": "am", "to": "out" }}{depth_wire}"#
        ),
    );
    let mut graph = compile(&json).unwrap();
    let input = sources(|_| 1.0, |i| i as f32 / 10.0);
    graph.process(0, &input).unwrap().data.clone()
}

#[test]
fn integer_parameters_are_rounded() {
    let out = am_with_integer_depth(r#""integer": ["depth"]"#, false);
    assert!(out.iter().all(|&x| x == 3.0), "{out:?}");
    // Without the option the fraction stays.
    let out = am_with_integer_depth(r#""bypass": false"#, false);
    assert!(out.iter().all(|&x| (x - 2.6).abs() < 1e-6), "{out:?}");
}

#[test]
fn modulated_integer_parameters_step_after_modulation() {
    let out = am_with_integer_depth(
        r#""integer": ["depth"], "modulation": { "depth": { "amount": 5 } }"#,
        true,
    );
    for (pixel, rgb) in out.chunks(3).enumerate() {
        // depth = round(1.6 + 5 × a), unipolar by default would equal this for a >= 0.
        let expected = 1.0 + (1.6 + 5.0 * f64::from(held_audio(pixel))).round() as f32;
        assert!((rgb[0] - expected).abs() < 1e-6, "pixel {pixel}: {rgb:?}");
    }
}

#[test]
fn integer_needs_a_number_parameter() {
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" },
           { "id": "d", "type": "distortion", "integer": ["shape"] },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "d" }, { "from": "d", "to": "out" }"#,
    );
    assert!(compile(&json).is_err());
}

#[test]
fn time_units_mean_the_same_together_and_separate() {
    // One row of RGB is 3 × W samples together and W per channel separately: either way a
    // one-row delay moves the picture down by exactly one row.
    let json = |channels: &str| {
        graph_json(
            &format!(
                r#"{{ "id": "video", "type": "video_input" }},
                   {{ "id": "d", "type": "delay", "params": {{ "time": 1, "unit": "rows" }}, "channels": "{channels}" }},
                   {{ "id": "out", "type": "output" }}"#
            ),
            r#"{ "from": "video", "to": "d" }, { "from": "d", "to": "out" }"#,
        )
    };
    let (mut together, mut separate) = (
        compile(&json("together")).unwrap(),
        compile(&json("separate")).unwrap(),
    );
    let row = (W * 3) as usize;
    for n in 0..3 {
        let input = sources(move |i| ((i + n * 7) % 11) as f32 / 10.0, |_| 0.0);
        let a = together.process(n as u64, &input).unwrap().data.clone();
        let b = separate.process(n as u64, &input).unwrap().data.clone();
        assert_eq!(a, b, "frame {n}");
        if n > 0 {
            // Row 1 of this frame is row 0 of the same frame's input.
            assert_eq!(a[row..2 * row], input["video"].data[..row]);
        }
    }
}

#[test]
fn inspect_covers_nodes_that_dont_feed_the_output() {
    use rastersong_graph::Part;
    // A Split of stereo audio with nothing plugged into it, a node downstream of a failure,
    // and the plain video-to-output chain.
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
           { "id": "split", "type": "split" }, { "id": "bad", "type": "pack", "params": { "channels": 7 } },
           { "id": "after", "type": "delay" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "audio", "to": "split" }, { "from": "video", "to": "bad" },
           { "from": "bad", "to": "after" }, { "from": "video", "to": "out" }"#,
    );
    let stats = Graph::inspect(
        &GraphDesc::from_json(&json).unwrap(),
        &Registry::default(),
        &stereo_options(4),
    );
    let of = |id: &str| stats.iter().find(|s| &*s.node == id);
    let split = of("split").expect("the orphan split is inspected");
    assert_eq!(split.inputs[0].samples_per_pixel, 2);
    assert_eq!(split.outputs[0].tag.part, Part::Left);
    assert_eq!(split.outputs[1].tag.part, Part::Right);
    assert!(of("out").is_some());
    // Pack can't make pixels of 7 channels, so it and what follows are left out.
    assert!(of("bad").is_none() && of("after").is_none());
}

#[test]
fn the_audio_output_carries_its_signal_alongside_the_picture() {
    // Gain on the audio, written to the audio output; the picture is passed through.
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
           { "id": "inv", "type": "invert", "params": { "mode": "audio" } },
           { "id": "sound", "type": "audio_output" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "out" }, { "from": "audio", "to": "inv" }, { "from": "inv", "to": "sound" }"#,
    );
    let mut graph = compile(&json).unwrap();
    assert!(
        graph
            .audio_layout()
            .unwrap()
            .same_shape(&Layout::audio(AUDIO))
    );
    assert_eq!(graph.audio_passthrough(), None);
    graph
        .process(0, &sources(|_| 0.0, |i| i as f32 / 10.0))
        .unwrap();
    let sound = graph.audio_output().unwrap();
    assert_eq!(sound.data, [0.0, -0.1, -0.2, -0.3, -0.4]);
}

#[test]
fn audio_outputs_without_input_or_processing_leave_the_source_audio_alone() {
    // Nothing connected: as if there were no audio output.
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "sound", "type": "audio_output" },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "out" }"#,
    );
    let graph = compile(&json).unwrap();
    assert!(graph.audio_layout().is_none());
    // A track wired straight in is passed through.
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
           { "id": "sound", "type": "audio_output" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "out" }, { "from": "audio", "to": "sound" }"#,
    );
    assert_eq!(compile(&json).unwrap().audio_passthrough(), Some("audio"));
    // Unconnected audio outputs don't change what renders.
    let with = render_form_of(&json, false);
    let extra = json.replace(
        r#"{ "id": "out", "type": "output" }"#,
        r#"{ "id": "out", "type": "output" }, { "id": "idle", "type": "audio_output" }"#,
    );
    assert_eq!(render_form_of(&extra, false), with);
}

#[test]
fn only_one_audio_output_and_nothing_reads_an_output() {
    let two = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
           { "id": "a", "type": "audio_output" }, { "id": "b", "type": "audio_output" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "out" }, { "from": "audio", "to": "a" }, { "from": "audio", "to": "b" }"#,
    );
    assert!(matches!(
        compile(&two),
        Err(GraphError::AudioOutputCount(2))
    ));
    let reads = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
           { "id": "a", "type": "audio_output" }, { "id": "d", "type": "delay" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "out" }, { "from": "audio", "to": "a" }, { "from": "a", "to": "d" }"#,
    );
    assert!(compile(&reads).is_err());
}

#[test]
fn both_outputs_line_up_at_the_later_latency() {
    // Half a frame of lookahead on the audio only: the picture is held back to match, so frame n
    // of the picture and of the sound both come from source frame n - 1.
    let mut registry = Registry::default();
    rastersong_graph::testing::register_fakes(&mut registry);
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
           { "id": "look", "type": "lookahead" }, { "id": "sound", "type": "audio_output" },
           { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "out" }, { "from": "audio", "to": "look" }, { "from": "look", "to": "sound" }"#,
    );
    let mut graph = compile_with(&json, &registry).unwrap();
    assert_eq!(graph.latency_frames(), 1);
    let frame = Layout::rgb(W, H).len();
    let audio = AUDIO as usize;
    let mut pictures = Vec::new();
    let mut sounds = Vec::new();
    for n in 0..4 {
        let input = sources(
            move |i| (n * frame + i) as f32,
            move |i| (n * audio + i) as f32,
        );
        pictures.push(graph.process(n as u64, &input).unwrap().data.clone());
        sounds.push(graph.audio_output().unwrap().data.clone());
    }
    for n in 1..4 {
        assert_eq!(pictures[n][0], ((n - 1) * frame) as f32, "picture {n}");
        assert_eq!(sounds[n][0], ((n - 1) * audio) as f32, "sound {n}");
    }
}

#[test]
fn odd_audio_output_layouts_are_written_as_stereo_with_a_warning() {
    let json = graph_json(
        r#"{ "id": "video", "type": "video_input" }, { "id": "a", "type": "to_audio" },
           { "id": "sound", "type": "audio_output" }, { "id": "out", "type": "output" }"#,
        r#"{ "from": "video", "to": "out" }, { "from": "video", "to": "a" }, { "from": "a", "to": "sound" }"#,
    );
    let graph = compile(&json).unwrap();
    let warnings = graph.diagnostics();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].message.contains("stereo"));
}
