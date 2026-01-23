//! Pipeline factory: builds tile-reading operation graphs from VPL.
//!
//! This module provides [`PipelineFactory`], a registry-driven builder that parses the
//! VersaTiles Pipeline Language (VPL) and constructs an executable chain of operations.
//! It wires together *read* and *transform* operation factories, resolves nested
//! container readers via a user-provided callback, and returns a boxed [`TileSource`]
//! ready to stream tiles.
//!
//! The factory can be instantiated empty (for custom registration) or with defaults that
//! register all built-in read/transform operations. For testing and demos there is also
//! a "dummy" mode that resolves filenames to synthetic vector/raster sources.

use crate::{
	helpers::{dummy_image_source::DummyImageSource, dummy_vector_source::DummyVectorSource},
	operations::{get_read_operation_factories, get_transform_operation_factories},
	vpl::{VPLNode, VPLPipeline, parse_vpl},
};
use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::BoxFuture;
use itertools::Itertools;
use regex::Regex;
use std::{
	collections::HashMap,
	path::{Path, PathBuf},
	sync::LazyLock,
	vec,
};
use versatiles_container::{TileSource, TilesRuntime};
use versatiles_core::{TileFormat, TileType};
use versatiles_derive::context;

static MULTIPLE_NEWLINES_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

pub trait OperationFactoryTrait: Send + Sync {
	fn get_tag_name(&self) -> &str;
	fn get_docs(&self) -> String;
}

/// Factory trait for read operations that create tile sources from VPL nodes.
///
/// Read operations are the entry points of a pipeline, creating tile sources
/// from files, databases, or other data sources.
#[async_trait]
pub trait ReadOperationFactoryTrait: OperationFactoryTrait {
	/// Build a tile source from a VPL node configuration.
	///
	/// Returns a boxed [`TileSource`] (which also implements [`TileSource`]
	/// via blanket implementation) that can be used as the start of a pipeline.
	async fn build<'a>(&self, vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn TileSource>>;
}

/// Factory trait for transform operations that wrap and modify existing tile sources.
///
/// Transform operations take an upstream tile source and apply transformations,
/// filtering, or other processing to the tiles.
#[async_trait]
pub trait TransformOperationFactoryTrait: OperationFactoryTrait {
	/// Build a transform operation that wraps an existing tile source.
	///
	/// Takes a source tile stream and VPL node configuration, returning a new
	/// tile source that applies the transformation.
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn TileSource>>;
}

/// Callback used to resolve a filename/URL into a concrete [`TileSource`].
///
/// The factory invokes this to open external containers referenced by VPL `read` nodes.
/// It receives the resolved path (relative to `dir`) and returns a boxed reader.
type Callback = Box<dyn Fn(String) -> BoxFuture<'static, Result<Box<dyn TileSource>>>>;

/// Builder that registers read/transform operation factories and produces an operation graph.
///
/// `PipelineFactory` maintains:
/// - `read_ops` and `tran_ops`: registries keyed by VPL tag name.
/// - `dir`: base directory used to resolve relative filenames.
/// - `create_reader`: callback to open external containers as [`TileSource`].
/// - `runtime`: runtime configuration forwarded to operations.
pub struct PipelineFactory {
	read_ops: HashMap<String, Box<dyn ReadOperationFactoryTrait>>,
	tran_ops: HashMap<String, Box<dyn TransformOperationFactoryTrait>>,
	dir: PathBuf,
	create_reader: Callback,
	runtime: TilesRuntime,
}

impl PipelineFactory {
	/// Creates an empty factory with no registered operations.
	pub fn new_empty(dir: &Path, create_reader: Callback, runtime: TilesRuntime) -> Self {
		PipelineFactory {
			read_ops: HashMap::new(),
			tran_ops: HashMap::new(),
			dir: dir.to_path_buf(),
			create_reader,
			runtime,
		}
	}

	/// Creates a factory pre-loaded with all built-in read and transform operation factories.
	pub fn new_default(dir: &Path, create_reader: Callback, runtime: TilesRuntime) -> Self {
		let mut factory = PipelineFactory::new_empty(dir, create_reader, runtime);

		for f in get_read_operation_factories() {
			factory.add_read_factory(f);
		}

		for f in get_transform_operation_factories() {
			factory.add_tran_factory(f);
		}

		factory
	}

	/// Creates a factory that resolves readers using a built-in dummy callback.
	///
	/// Useful for examples and tests: resolves vector sources to `DummyVectorSource` and
	/// raster sources to `DummyImageSource` based on the filename’s extension/color code.
	pub fn new_dummy() -> Self {
		PipelineFactory::new_dummy_reader(Box::new(|filename: String| -> BoxFuture<Result<Box<dyn TileSource>>> {
			Box::pin(async move {
				let mut name = filename.clone();
				let format = TileFormat::from_filename(&mut name)
					.ok_or_else(|| anyhow!("cannot determine tile format from filename '{filename}'"))?;

				Ok(match format.to_type() {
					TileType::Vector => Box::new(DummyVectorSource::new(
						&[("dummy", &[&[("filename", &filename)]])],
						None,
					)) as Box<dyn TileSource>,
					TileType::Raster => {
						let color = if !name.is_empty() && name.len() <= 4 {
							#[allow(clippy::cast_possible_truncation)]
							name
								.chars()
								.filter_map(|c| c.to_digit(16).map(|d| (d * 17) as u8))
								.collect()
						} else {
							vec![50, 150, 250]
						};
						Box::new(DummyImageSource::from_color(&color, 4, format, None).unwrap()) as Box<dyn TileSource>
					}
					_ => bail!("unsupported tile type for dummy reader in filename '{filename}'"),
				})
			})
		}))
	}

	/// Creates a default-registered factory using the provided custom reader callback.
	pub fn new_dummy_reader(create_reader: Callback) -> Self {
		#[cfg(not(test))]
		let runtime = TilesRuntime::default();
		#[cfg(test)]
		let runtime = TilesRuntime::new_silent();

		PipelineFactory::new_default(Path::new(""), create_reader, runtime)
	}

	/// Registers a read operation factory under its VPL tag name.
	fn add_read_factory(&mut self, factory: Box<dyn ReadOperationFactoryTrait>) {
		self.read_ops.insert(factory.get_tag_name().to_string(), factory);
	}

	/// Registers a transform operation factory under its VPL tag name.
	fn add_tran_factory(&mut self, factory: Box<dyn TransformOperationFactoryTrait>) {
		self.tran_ops.insert(factory.get_tag_name().to_string(), factory);
	}

	/// Resolves `filename` relative to `dir` and invokes `create_reader` to open a container.
	#[context("Failed to get reader for file '{}'", filename)]
	pub async fn get_reader(&self, filename: &str) -> Result<Box<dyn TileSource>> {
		(self.create_reader.as_ref())(self.dir.join(filename).to_string_lossy().to_string()).await
	}

	/// Parses VPL text and builds the corresponding operation graph.
	#[context("Failed to create reader from VPL")]
	pub async fn operation_from_vpl(&self, text: &str) -> Result<Box<dyn TileSource>> {
		self.build_pipeline(parse_vpl(text)?).await
	}

	/// Builds an executable operation graph from a parsed `VPLPipeline`.
	///
	/// Takes the head node as a read operation and folds the remaining nodes as transforms.
	#[context("Failed to build pipeline from VPL")]
	pub async fn build_pipeline(&self, pipeline: VPLPipeline) -> Result<Box<dyn TileSource>> {
		let (head, tail) = pipeline.split()?;

		let mut vpl_operation = self.read_operation_from_node(head).await?;

		for node in tail {
			vpl_operation = self.tran_operation_from_node(node, vpl_operation).await?;
		}

		Ok(vpl_operation)
	}

	/// Instantiates a read operation from a VPL node using the registered factory.
	#[context("Failed to create read operation from VPL node")]
	async fn read_operation_from_node(&self, node: VPLNode) -> Result<Box<dyn TileSource>> {
		let factory = self
			.read_ops
			.get(&node.name)
			.ok_or_else(|| anyhow!("read operation '{}' unknown", node.name))?;

		factory.build(node, self).await
	}

	/// Instantiates a transform operation from a VPL node using the registered factory.
	#[context("Failed to create transform operation from VPL node")]
	async fn tran_operation_from_node(&self, node: VPLNode, source: Box<dyn TileSource>) -> Result<Box<dyn TileSource>> {
		let factory = self
			.tran_ops
			.get(&node.name)
			.ok_or_else(|| anyhow!("transform operation '{}' unknown", node.name))?;

		factory.build(node, source, self).await
	}

	/// Returns the absolute/normalized string path for a VPL-referenced `filename`.
	pub fn resolve_filename(&self, filename: &str) -> String {
		String::from(self.resolve_path(filename).to_str().unwrap())
	}

	/// Resolves a VPL-referenced `filename` against `dir` and returns a `PathBuf`.
	pub fn resolve_path(&self, filename: &str) -> PathBuf {
		self.dir.join(filename)
	}

	/// Returns rendered Markdown help listing all registered operations and their docs.
	pub fn help_md(&self) -> String {
		#[allow(clippy::borrowed_box, clippy::needless_pass_by_value)]
		fn to_md<T>(vec: Vec<&Box<T>>) -> String
		where
			T: OperationFactoryTrait + ?Sized,
		{
			vec.iter()
				.sorted_by_key(|f| f.get_tag_name())
				.map(|f| format!("## {}\n\n{}", f.get_tag_name(), f.get_docs()))
				.join("\n\n")
		}

		let doc = [
			include_str!("help.md").to_string(),
			String::from("---"),
			String::from("# READ operations"),
			to_md(self.read_ops.values().collect_vec()),
			String::from("---"),
			String::from("# TRANSFORM operations"),
			to_md(self.tran_ops.values().collect_vec()),
		]
		.join("\n\n");

		MULTIPLE_NEWLINES_REGEX.replace_all(&doc, "\n\n").to_string()
	}

	/// Returns the runtime associated with this factory.
	pub fn runtime(&self) -> TilesRuntime {
		self.runtime.clone()
	}
}

unsafe impl Sync for PipelineFactory {}
unsafe impl Send for PipelineFactory {}
