use anyhow::{ensure, Result};

use super::VPLNode;

#[derive(Clone, Debug, PartialEq)]
pub struct VPLPipeline {
	pub pipeline: Vec<VPLNode>,
}

impl VPLPipeline {
	pub fn split(mut self) -> Result<(VPLNode, Vec<VPLNode>)> {
		ensure!(!self.pipeline.is_empty(), "pipeline is empty");
		let first_element = self.pipeline.remove(0);
		Ok((first_element, self.pipeline))
	}
}

impl From<Vec<VPLNode>> for VPLPipeline {
	fn from(pipeline: Vec<VPLNode>) -> Self {
		VPLPipeline { pipeline }
	}
}

impl From<VPLNode> for VPLPipeline {
	fn from(node: VPLNode) -> Self {
		VPLPipeline {
			pipeline: vec![node],
		}
	}
}