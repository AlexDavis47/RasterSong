//! Throughput of each effect node on a 1080p frame, and of whole example graphs.
//! Run with `cargo bench -p rastersong-graph`.

use std::collections::{BTreeMap, HashMap};
use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rastersong_graph::{
    CompileOptions, Graph, GraphDesc, Layout, ParamValue, PrepareContext, ProcessContext, Registry,
    Signal, Sources,
};

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const FPS: f64 = 30.0;
/// 48 kHz audio at 30 fps.
const AUDIO_BLOCK: u32 = 1600;

struct NoSources;

impl Sources for NoSources {
    fn get(&self, _: &str) -> Option<&Signal> {
        None
    }
}

fn ramp(len: usize, scale: f32) -> Vec<f32> {
    (0..len)
        .map(|i| ((i * 7919) % 1000) as f32 / 1000.0 * scale)
        .collect()
}

fn nodes(c: &mut Criterion) {
    let layout = Layout::mono(WIDTH, HEIGHT);
    let mut group = c.benchmark_group("node, one 1080p channel");
    group.throughput(Throughput::Elements(layout.len() as u64));
    let registry = Registry::shared();
    for t in registry.types() {
        // Nodes declare their benchmark configuration (`NodeKind::BENCH`); others are skipped.
        let Some(params) = t.bench else { continue };
        let (name, kind) = (t.kind.as_str(), t.kind.as_str());
        let params: BTreeMap<String, ParamValue> = serde_json::from_str(params).unwrap();
        let mut node = registry.create(kind, &params).unwrap().unwrap();
        let (inputs, outputs) = (t.spec.inputs.len(), t.spec.outputs.len());
        node.prepare(&PrepareContext {
            frame_rate: FPS,
            tempo: Default::default(),
            inputs: &vec![layout; inputs],
            outputs: &vec![layout; outputs],
            connected: &vec![true; inputs],
            modulated: &[],
            pixel_scale: 1.0,
        });
        let signals = [
            Signal::from_data(layout, ramp(layout.len(), 1.0)),
            Signal::from_data(layout, ramp(layout.len(), 2.0)),
        ];
        let refs: Vec<&Signal> = signals[..inputs].iter().collect();
        let mut out = vec![Signal::zeros(layout); outputs];
        let ctx = ProcessContext {
            frame: 0,
            frame_rate: FPS,
            sources: &NoSources,
            params: &[],
        };
        group.bench_function(name, |b| {
            b.iter(|| node.process(&ctx, black_box(&refs), &mut out));
        });
    }
    group.finish();
}

fn graphs(c: &mut Criterion) {
    let video = Layout::rgb(WIDTH, HEIGHT);
    let audio = Layout::audio(AUDIO_BLOCK);
    let sources = HashMap::from([
        (
            "video".to_owned(),
            Signal::from_data(video, ramp(video.len(), 1.0)),
        ),
        (
            "audio".to_owned(),
            Signal::from_data(audio, ramp(audio.len(), 2.0)),
        ),
    ]);
    let mut group = c.benchmark_group("example graph, 1080p frames");
    group.throughput(Throughput::Elements(1));
    group.sample_size(20);
    for name in ["am_bands", "bass_wave", "packed_crush"] {
        let path = format!(
            "{}/../../examples/graphs/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let desc = GraphDesc::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut graph = Graph::compile(
            &desc,
            Registry::shared(),
            &CompileOptions {
                frame_rate: FPS,
                tempo: Default::default(),
                sources: HashMap::from([("video".to_owned(), video), ("audio".to_owned(), audio)]),
                output: video,
                pixel_scale: 1.0,
            },
        )
        .unwrap();
        let mut frame = 0;
        group.bench_function(name, |b| {
            b.iter(|| {
                frame += 1;
                black_box(graph.process(frame, &sources).unwrap());
            });
        });
    }
    group.finish();
}

criterion_group!(benches, nodes, graphs);
criterion_main!(benches);
