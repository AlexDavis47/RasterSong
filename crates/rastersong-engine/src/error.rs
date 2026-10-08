use rastersong_graph::GraphError;
use rastersong_lang::tr_args;
use rastersong_media::MediaError;

use crate::timeline::Timebase;

#[derive(Debug, Clone, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Graph(#[from] GraphError),
    /// Raised by an output, e.g. when writing a file fails.
    #[error("{0}")]
    Output(String),
    /// A graph placed on a layer can't be compiled.
    #[error("{}", layer_message(*.graph, .error))]
    Layer { graph: u32, error: GraphError },
    /// The project's size or frame rate can't be rendered.
    #[error("{}", timebase_message(.0))]
    Timebase(Timebase),
}

fn layer_message(graph: u32, error: &GraphError) -> String {
    tr_args(
        "error.project.layer_graph",
        &[("graph", &graph.to_string()), ("error", &error.to_string())],
    )
}

fn timebase_message(timebase: &Timebase) -> String {
    tr_args(
        "error.project.timebase",
        &[
            ("width", &timebase.width.to_string()),
            ("height", &timebase.height.to_string()),
            (
                "rate",
                &format!("{}/{}", timebase.frame_rate.num, timebase.frame_rate.den),
            ),
        ],
    )
}
