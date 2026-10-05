#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GraphError {
    #[error("invalid graph file: {0}")]
    Parse(String),

    #[error("node `{id}`: unknown node type `{kind}`")]
    UnknownNodeType { id: String, kind: String },

    #[error("node id `{0}` is used more than once")]
    DuplicateId(String),

    #[error("connection `{connection}`: {message}")]
    Connection { connection: String, message: String },

    #[error("node `{node}`: input `{input}` must be connected")]
    MissingInput { node: String, input: String },

    #[error("the graph has a cycle through {0:?}")]
    Cycle(Vec<String>),

    #[error("the graph must have exactly one `output` node, found {0}")]
    OutputCount(usize),

    #[error("the graph can have at most one `audio_output` node, found {0}")]
    AudioOutputCount(usize),

    #[error("node `{node}`: {message}")]
    Node { node: String, message: String },

    #[error("source `{name}`: {message}")]
    Source { name: String, message: String },
}

impl GraphError {
    /// The node the error is about, if it's about one node.
    pub fn node(&self) -> Option<&str> {
        match self {
            Self::UnknownNodeType { id, .. } | Self::DuplicateId(id) => Some(id),
            Self::MissingInput { node, .. } | Self::Node { node, .. } => Some(node),
            Self::Cycle(nodes) => nodes.first().map(String::as_str),
            Self::Parse(_)
            | Self::Connection { .. }
            | Self::OutputCount(_)
            | Self::AudioOutputCount(_)
            | Self::Source { .. } => None,
        }
    }
}
