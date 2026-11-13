//! Comment and Note: nodes that only annotate the graph. They have no ports and process
//! nothing; [`crate::without_annotations`] takes them out before a graph is compiled. Their title
//! is the node's `label`, their text its `description`, and their box the node's `size`.

use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Layout, LayoutContext, Node, Params, ProcessContext, Signal};

/// A box behind a group of nodes, with a title and a description. Moving it moves the nodes
/// inside it.
#[derive(Debug)]
pub struct Comment;

/// A sticky note showing its text, optionally attached to another node, which it then follows
/// and points at.
#[derive(Debug)]
pub struct StickyNote;

/// What both annotations are: no ports, no parameters.
const SPEC: NodeSpec = NodeSpec::new(Category::Annotation).inputs(&[]).outputs(&[]);

impl NodeKind for Comment {
    const KIND: &'static str = crate::nodes::COMMENT;
    const SPEC: NodeSpec = SPEC;

    fn new(_params: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl NodeKind for StickyNote {
    const KIND: &'static str = crate::nodes::NOTE;
    const SPEC: NodeSpec = SPEC;

    fn new(_params: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl Node for Comment {
    fn output_layouts(&self, _ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        Ok(Vec::new())
    }

    fn process(&mut self, _ctx: &ProcessContext, _inputs: &[&Signal], _outputs: &mut [Signal]) {}
}

impl Node for StickyNote {
    fn output_layouts(&self, _ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        Ok(Vec::new())
    }

    fn process(&mut self, _ctx: &ProcessContext, _inputs: &[&Signal], _outputs: &mut [Signal]) {}
}
