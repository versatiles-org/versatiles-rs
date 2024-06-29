use anyhow::{ensure, Result};

use super::VDLNode;

#[derive(Clone, Debug, PartialEq)]
pub struct VDLPipeline {
	pub pipeline: Vec<VDLNode>,
}

impl VDLPipeline {
	pub fn split(mut self) -> Result<(VDLNode, Vec<VDLNode>)> {
		ensure!(!self.pipeline.is_empty(), "pipeline is empty");
		let first_element = self.pipeline.remove(0);
		Ok((first_element, self.pipeline))
	}
}

impl From<Vec<VDLNode>> for VDLPipeline {
	fn from(pipeline: Vec<VDLNode>) -> Self {
		VDLPipeline { pipeline }
	}
}

impl From<VDLNode> for VDLPipeline {
	fn from(node: VDLNode) -> Self {
		VDLPipeline {
			pipeline: vec![node],
		}
	}
}
