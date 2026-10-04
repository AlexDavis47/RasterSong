use rastersong_graph::GraphError;
use rastersong_media::MediaError;

#[derive(Debug, Clone, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Graph(#[from] GraphError),
    /// Raised by an output, e.g. when writing a file fails.
    #[error("{0}")]
    Output(String),
}
