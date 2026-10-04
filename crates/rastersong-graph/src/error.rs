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

    #[error("node `{node}`: {message}")]
    Node { node: String, message: String },

    #[error("source `{name}`: {message}")]
    Source { name: String, message: String },
}
