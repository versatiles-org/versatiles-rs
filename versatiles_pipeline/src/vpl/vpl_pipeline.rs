use std::fmt::Debug;

use anyhow::{ensure, Result};

use super::{parse_vpl, VPLNode};

#[derive(Clone, PartialEq)]
pub struct VPLPipeline {
	pub pipeline: Vec<VPLNode>,
}

impl VPLPipeline {
	pub fn new(pipeline: Vec<VPLNode>) -> Self {
		VPLPipeline { pipeline }
	}
	pub fn from_str(vpl: &str) -> Self {
		parse_vpl(vpl).unwrap()
	}

	pub fn len(&self) -> usize {
		self.pipeline.len()
	}

	pub fn is_empty(&self) -> bool {
		self.pipeline.is_empty()
	}

	pub fn pop(&mut self) -> Option<VPLNode> {
		self.pipeline.pop()
	}

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

impl Debug for VPLPipeline {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.pipeline).finish()
	}
}
