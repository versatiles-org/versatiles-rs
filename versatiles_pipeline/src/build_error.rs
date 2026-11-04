//! Which node a pipeline failed to build at.

use std::fmt;

/// The node a pipeline failed to build at, attached to the build error.
///
/// Building a pipeline can fail at any node: a file that isn't there, a source
/// that holds no elevation, a format a transform refuses. The error says what
/// went wrong, and this says where, with the same [`path`](Self::path) a
/// [`VplProblem`](crate::VplProblem) carries. Get it back with
/// [`anyhow::Error::downcast_ref`]:
///
/// ```
/// use versatiles_container::TilesRuntime;
/// use versatiles_pipeline::{NodeBuildError, PipelineReader};
///
/// # #[tokio::main]
/// # async fn main() {
/// let vpl = "from_debug format=png | raster_format format=avif quality=100";
/// let dir = std::path::Path::new(".");
/// let err = PipelineReader::open_str(vpl, dir, TilesRuntime::default()).await.unwrap_err();
/// let node = err.downcast_ref::<NodeBuildError>().unwrap();
/// assert_eq!(node.path, [1]);
/// assert_eq!(node.name, "raster_format");
/// # }
/// ```
///
/// Its `Display` reads like `node 1 (raster_format)`, so the plain error
/// message names the node too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeBuildError {
	/// Which node, as a path of indices from the root pipeline.
	///
	/// As in [`VplProblem::path`](crate::VplProblem::path): `[2]` is the third
	/// node of the pipeline, and `[0, 1, 2]` is the third node of the second
	/// bracketed source of the first node.
	pub path: Vec<usize>,
	/// The operation of that node, such as `raster_format`.
	pub name: String,
	/// Set once the pipeline the path is relative to is complete. A pipeline
	/// read from another VPL file starts a path of its own, which the nodes
	/// around it must not extend.
	complete: bool,
}

impl fmt::Display for NodeBuildError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let mut steps = self.path.iter();
		if let Some(node) = steps.next() {
			write!(f, "node {node}")?;
		}
		while let (Some(source), Some(node)) = (steps.next(), steps.next()) {
			write!(f, ", source {source}, node {node}")?;
		}
		write!(f, " ({})", self.name)
	}
}

/// Places a failure of the node at `index`, called `name`, in its pipeline.
///
/// A failure inside a bracketed source already names its node; the path then
/// gains this node's index in front. Any other failure gets a new
/// [`NodeBuildError`].
pub(crate) fn at_node(mut err: anyhow::Error, index: usize, name: &str) -> anyhow::Error {
	if let Some(inner) = err.downcast_mut::<NodeBuildError>()
		&& !inner.complete
	{
		inner.path.insert(0, index);
		return err;
	}
	err.context(NodeBuildError {
		path: vec![index],
		name: name.to_string(),
		complete: false,
	})
}

/// Places a failure of a node's bracketed source at `index` in that node.
pub(crate) fn in_source(mut err: anyhow::Error, index: usize) -> anyhow::Error {
	if let Some(inner) = err.downcast_mut::<NodeBuildError>()
		&& !inner.complete
	{
		inner.path.insert(0, index);
	}
	err
}

/// Marks the path as complete: the pipeline it is relative to is the root.
pub(crate) fn complete(mut err: anyhow::Error) -> anyhow::Error {
	if let Some(inner) = err.downcast_mut::<NodeBuildError>() {
		inner.complete = true;
	}
	err
}

#[cfg(test)]
mod tests {
	use super::*;

	fn node(path: &[usize], name: &str) -> NodeBuildError {
		NodeBuildError {
			path: path.to_vec(),
			name: name.to_string(),
			complete: false,
		}
	}

	#[test]
	fn display_names_each_level() {
		assert_eq!(node(&[2], "raster_format").to_string(), "node 2 (raster_format)");
		assert_eq!(
			node(&[0, 1, 2], "dem_hillshade").to_string(),
			"node 0, source 1, node 2 (dem_hillshade)"
		);
	}

	#[test]
	fn a_nested_failure_gains_the_path_of_its_parents() {
		let err = at_node(anyhow::anyhow!("no such file"), 2, "from_container");
		let err = in_source(err, 1);
		let err = at_node(err, 0, "from_stacked_raster");
		let found = err.downcast_ref::<NodeBuildError>().unwrap();
		assert_eq!(found.path, [0, 1, 2]);
		assert_eq!(
			found.name, "from_container",
			"the innermost node is the one that failed"
		);
	}

	#[test]
	fn a_complete_path_is_not_extended() {
		let err = complete(at_node(anyhow::anyhow!("no such file"), 2, "from_container"));
		let err = at_node(err, 0, "from_container");
		let outer = err.downcast_ref::<NodeBuildError>().unwrap();
		assert_eq!(outer.path, [0], "the node that read the other VPL file");
	}
}
