use std::collections::HashMap;

use rastersong_graph::{
    CompileOptions, Graph, GraphDesc, GraphError, InputSpec, Layout, Node, PrepareContext,
    ProcessContext, Registry, Signal,
};

const W: u32 = 4;
const H: u32 = 2;
const AUDIO: u32 = 5;

fn options() -> CompileOptions {
    CompileOptions {
        frame_rate: 30.0,
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
        (r#"{ "id": "o", "type": "output" }"#, "", |e| {
            matches!(e, GraphError::MissingInput { .. })
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

/// Test node: delays its input by `samples` and reports that as latency, like a node that
/// needs lookahead would.
struct Lookahead {
    samples: usize,
    history: Vec<f32>,
}

impl Node for Lookahead {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }
    fn process(&mut self, _: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        for (out, &x) in outputs[0].data.iter_mut().zip(&inputs[0].data) {
            self.history.push(x);
            let n = self.history.len();
            *out = if n > self.samples {
                self.history[n - 1 - self.samples]
            } else {
                0.0
            };
        }
    }
    fn reset(&mut self) {
        self.history.clear();
    }
    fn latency(&self, _: &PrepareContext) -> usize {
        self.samples
    }
}

/// Test node: adds its two inputs.
struct Sum;

impl Node for Sum {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("a"), InputSpec::required("b")];
        INPUTS
    }
    fn process(&mut self, _: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        for ((out, &a), &b) in outputs[0]
            .data
            .iter_mut()
            .zip(&inputs[0].data)
            .zip(&inputs[1].data)
        {
            *out = a + b;
        }
    }
}

#[test]
fn latency_is_compensated_across_branches() {
    // Half a frame of lookahead on one branch only. Without compensation, `sum` would add two
    // signals half a frame apart. The output is rounded up to a whole frame of latency.
    let frame = Layout::rgb(W, H).len();
    let mut registry = Registry::default();
    registry.register("lookahead", move |_| {
        Ok(Lookahead {
            samples: frame / 2,
            history: Vec::new(),
        })
    });
    registry.register("sum", |_| Ok(Sum));
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
        GraphDesc::from_json(r#"{ "version": 2, "nodes": [] }"#),
        Err(GraphError::Parse(_))
    ));
}
