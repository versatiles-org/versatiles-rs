use super::{VPLNode, parse_vpl};
use anyhow::{Result, ensure};
use std::fmt::Debug;
use versatiles_derive::context;

#[derive(Clone, Default, PartialEq)]
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

	#[context("Failed to split VPL pipeline")]
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
		VPLPipeline { pipeline: vec![node] }
	}
}

impl Debug for VPLPipeline {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.pipeline).finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn make_node(name: &str) -> VPLNode {
		VPLNode::try_from_str(name).unwrap()
	}

	#[test]
	fn test_new() {
		let nodes = vec![make_node("foo"), make_node("bar")];
		let pipeline = VPLPipeline::new(nodes);
		assert_eq!(pipeline.len(), 2);
	}

	#[test]
	fn test_from_str() {
		let pipeline = VPLPipeline::from_str("foo | bar | baz");
		assert_eq!(pipeline.len(), 3);
	}

	#[test]
	fn test_len() {
		let pipeline = VPLPipeline::new(vec![make_node("a"), make_node("b"), make_node("c")]);
		assert_eq!(pipeline.len(), 3);
	}

	#[test]
	fn test_is_empty() {
		let empty = VPLPipeline::new(vec![]);
		assert!(empty.is_empty());

		let non_empty = VPLPipeline::new(vec![make_node("foo")]);
		assert!(!non_empty.is_empty());
	}

	#[test]
	fn test_pop() {
		let mut pipeline = VPLPipeline::from_str("first | second | third");
		let popped = pipeline.pop();
		assert!(popped.is_some());
		assert_eq!(popped.unwrap().name, "third");
		assert_eq!(pipeline.len(), 2);
	}

	#[test]
	fn test_pop_empty() {
		let mut pipeline = VPLPipeline::new(vec![]);
		assert!(pipeline.pop().is_none());
	}

	#[test]
	fn test_split() {
		let pipeline = VPLPipeline::from_str("first | second | third");
		let (first, rest) = pipeline.split().unwrap();
		assert_eq!(first.name, "first");
		assert_eq!(rest.len(), 2);
		assert_eq!(rest[0].name, "second");
		assert_eq!(rest[1].name, "third");
	}

	#[test]
	fn test_split_empty_fails() {
		let pipeline = VPLPipeline::new(vec![]);
		let result = pipeline.split();
		assert!(result.is_err());
	}

	#[test]
	fn test_from_vec() {
		let nodes = vec![make_node("a"), make_node("b")];
		let pipeline: VPLPipeline = nodes.into();
		assert_eq!(pipeline.len(), 2);
	}

	#[test]
	fn test_from_node() {
		let node = make_node("single");
		let pipeline: VPLPipeline = node.into();
		assert_eq!(pipeline.len(), 1);
	}

	#[test]
	fn test_debug() {
		let pipeline = VPLPipeline::from_str("foo | bar");
		let debug_str = format!("{pipeline:?}");
		assert!(debug_str.contains("foo"));
		assert!(debug_str.contains("bar"));
	}

	#[test]
	fn test_clone() {
		let pipeline = VPLPipeline::from_str("foo | bar");
		let cloned = pipeline.clone();
		assert_eq!(pipeline, cloned);
	}

	#[test]
	fn test_default() {
		let pipeline = VPLPipeline::default();
		assert!(pipeline.is_empty());
	}

	#[test]
	fn test_partial_eq() {
		let pipeline1 = VPLPipeline::from_str("foo | bar");
		let pipeline2 = VPLPipeline::from_str("foo | bar");
		let pipeline3 = VPLPipeline::from_str("foo | baz");
		assert_eq!(pipeline1, pipeline2);
		assert_ne!(pipeline1, pipeline3);
	}
}
