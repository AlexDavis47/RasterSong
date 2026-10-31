use crate::nodes::{Category, NAME_PARAM, NAMED_OUTPUT, NodeKind, NodeSpec};
use crate::{InputSpec, Node, OutputSpec, ParamSpec, Params, ProcessContext, Signal};

/// An output beside the main ones, which other graphs read by its name (a Video Source or Audio
/// Source reading this graph's track). It adds nothing to the graph's own picture or sound.
/// Accepts any signal and passes it on as it is. Names are unique within a graph.
#[derive(Debug)]
pub struct NamedOutput;

params! { NamedOutput {
    NAME: ParamSpec::text(NAME_PARAM, "Out"),
} }

impl NodeKind for NamedOutput {
    const KIND: &'static str = NAMED_OUTPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Output)
        .params(Self::PARAMS)
        .inputs(&[InputSpec::required("in")])
        .outputs(&[OutputSpec::new("out")])
        .addable();

    fn new(_params: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl Node for NamedOutput {
    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        outputs[0].data.copy_from_slice(&inputs[0].data);
    }
}
