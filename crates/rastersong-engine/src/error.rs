use rastersong_graph::GraphError;
use rastersong_lang::tr_args;
use rastersong_media::MediaError;

use crate::project::ResourceId;
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
    /// An audio track's file can't be decoded.
    #[error("{}", tr_args("error.audio_track", &[("name", .name), ("error", &.error.to_string())]))]
    AudioTrack { name: String, error: MediaError },
    /// The render was stopped before it finished.
    #[error("{}", rastersong_lang::tr("error.render.cancelled"))]
    Cancelled,
    /// A graph placed on a graph track can't be compiled or fails. `name` is the graph
    /// resource's.
    #[error("{}", fx_message(.name, .error))]
    Fx {
        graph: ResourceId,
        name: String,
        error: GraphError,
    },
    /// The project's size or frame rate can't be rendered.
    #[error("{}", timebase_message(.0))]
    Timebase(Timebase),
}

fn fx_message(graph: &str, error: &GraphError) -> String {
    tr_args(
        "error.project.fx_graph",
        &[("graph", graph), ("error", &error.to_string())],
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
