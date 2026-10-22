use rastersong_lang::tr_args;

/// What can go wrong loading or compiling a graph. The text comes from the lang files
/// (`error.*`), so it follows the language in use.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphError {
    Parse(String),
    UnknownNodeType {
        id: String,
        kind: String,
    },
    DuplicateId(String),
    Connection {
        connection: String,
        message: String,
    },
    MissingInput {
        node: String,
        input: String,
    },
    Cycle(Vec<String>),
    OutputCount(usize),
    /// More than one audio output writes to `bus`.
    AudioOutputCount {
        bus: String,
        count: usize,
    },
    Node {
        node: String,
        message: String,
    },
    Source {
        name: String,
        message: String,
    },
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Parse(message) => tr_args("error.graph.parse", &[("message", message)]),
            Self::UnknownNodeType { id, kind } => tr_args(
                "error.graph.unknown_node_type",
                &[("id", id), ("kind", kind)],
            ),
            Self::DuplicateId(id) => tr_args("error.graph.duplicate_id", &[("id", id)]),
            Self::Connection {
                connection,
                message,
            } => tr_args(
                "error.graph.connection",
                &[("connection", connection), ("message", message)],
            ),
            Self::MissingInput { node, input } => tr_args(
                "error.graph.missing_input",
                &[("node", node), ("input", input)],
            ),
            Self::Cycle(nodes) => tr_args("error.graph.cycle", &[("nodes", &format!("{nodes:?}"))]),
            Self::OutputCount(n) => {
                tr_args("error.graph.output_count", &[("count", &n.to_string())])
            }
            Self::AudioOutputCount { bus, count } => tr_args(
                "error.graph.audio_output_count",
                &[("bus", bus), ("count", &count.to_string())],
            ),
            Self::Node { node, message } => {
                tr_args("error.graph.node", &[("node", node), ("message", message)])
            }
            Self::Source { name, message } => tr_args(
                "error.graph.source",
                &[("name", name), ("message", message)],
            ),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for GraphError {}

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
            | Self::AudioOutputCount { .. }
            | Self::Source { .. } => None,
        }
    }

    /// The error's text without the "node `id`: " lead-in, for showing under that node's name.
    pub fn detail(&self) -> String {
        match self {
            Self::Node { message, .. } => message.clone(),
            Self::MissingInput { input, .. } => {
                tr_args("error.graph.missing_input.detail", &[("input", input)])
            }
            Self::UnknownNodeType { kind, .. } => {
                tr_args("error.graph.unknown_node_type.detail", &[("kind", kind)])
            }
            other => other.to_string(),
        }
    }
}
