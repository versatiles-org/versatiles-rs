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

use std::{collections::HashMap, path::PathBuf, sync::LazyLock, time::Instant, vec};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::BoxFuture;
use itertools::Itertools;
use regex::Regex;
use versatiles_container::{DataLocation, TileSource, TilesRuntime};
use versatiles_core::{TileFormat, TileType};
use versatiles_derive::context;

use crate::{
	helpers::{dummy_image_source::DummyImageSource, dummy_vector_source::DummyVectorSource},
	operations::{read_operation_factories, transform_operation_factories},
	vpl::{VPLNode, VPLPipeline, parse_vpl},
};

static MULTIPLE_NEWLINES_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").expect("valid regex literal"));

/// What every VPL operation exposes to the registry, the reference generator
/// and `versatiles help`.
pub trait OperationFactoryTrait: Send + Sync {
	/// The operation's name as written in VPL, e.g. `from_container`.
	fn tag_name(&self) -> &str;
	/// The operation's full documentation, including the generated parameter
	/// list. This is what the operation reference and `versatiles help` render.
	fn docs(&self) -> String;
	/// The first paragraph of [`docs`](Self::docs), as one sentence.
	#[cfg(feature = "codegen")]
	fn doc_summary(&self) -> String;
	/// [`docs`](Self::docs) minus the summary and the generated parameter list.
	#[cfg(feature = "codegen")]
	fn doc_details(&self) -> String;
	/// One entry per argument the operation accepts.
	fn field_metadata(&self) -> Vec<crate::vpl::VPLFieldMeta>;
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

/// Whether a transform can be applied to a particular source.
///
/// The verdict is a *check*, not an estimate: [`Wrong`](Compatibility::Wrong)
/// means the operation looked and found a reason, and anything it cannot
/// demonstrate is [`Fits`](Compatibility::Fits). `dem_quantize` on an RGB PNG is
/// `Fits`, because nothing distinguishes terrarium elevation from a photograph —
/// a third "cannot tell" verdict would be offered exactly like `Fits` in a
/// picker, so it would earn nothing.
///
/// The reason sits on the variant that needs one. An `Option<String>` beside a
/// verdict would permit a `Wrong` with nothing to show, which is the one case a
/// UI cannot render.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Compatibility {
	/// No reason was found to rule this operation out.
	Fits,
	/// Checked and refused, e.g. "`raster_flatten` needs raster tiles; this
	/// source is mvt".
	Wrong(String),
}

impl Compatibility {
	/// Whether this operation may be offered for the source.
	#[must_use]
	pub fn fits(&self) -> bool {
		matches!(self, Compatibility::Fits)
	}

	/// Why the operation was refused, or `None` when it fits.
	#[must_use]
	pub fn reason(&self) -> Option<&str> {
		match self {
			Compatibility::Fits => None,
			Compatibility::Wrong(reason) => Some(reason),
		}
	}
}

/// Refuses `source` unless its tiles are of `required` type.
///
/// The check every format-bound operation needs, so the wording of the refusal
/// is written once. Used by `define_transform_factory!`'s `requires:` clause.
#[must_use]
pub fn require_tile_type(source: &dyn TileSource, required: TileType, tag: &str) -> Compatibility {
	let format = *source.metadata().tile_format();
	if format.to_type() == required {
		Compatibility::Fits
	} else {
		Compatibility::Wrong(format!(
			"`{tag}` needs {} tiles; this source is {format}",
			required.as_str()
		))
	}
}

/// Turns a refusal into the error a build should fail with.
///
/// Called by `define_transform_factory!` and by the hand-written factories that
/// do not go through it, so "the verdict and the build agree" is one fact with
/// one implementation rather than a convention each factory re-honours.
///
/// Belongs at the very top of `build`, before the node's parameters are read: an
/// operation that cannot apply to this source should say so, not complain about
/// a missing argument the caller had no reason to supply.
pub fn check_compatibility(verdict: Compatibility) -> Result<()> {
	match verdict {
		Compatibility::Fits => Ok(()),
		Compatibility::Wrong(reason) => bail!(reason),
	}
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

	/// Whether this operation could be applied to `source`.
	///
	/// Asked before any parameter is typed, so it takes `&self` on the singleton
	/// factory rather than needing an instance — `vector_filter_layers` is not
	/// constructible without `layers=`.
	///
	/// Defaults to [`Compatibility::Fits`], so operations adopt this one at a
	/// time and a partial migration cannot hide one that would have worked.
	/// `async` only for the few checks that need to read a tile; the format-based
	/// ones are synchronous and borrow, so they cost nothing.
	async fn compatibility(&self, _source: &dyn TileSource) -> Compatibility {
		Compatibility::Fits
	}
}

/// Callback used to resolve a filename/URL into a concrete [`TileSource`].
///
/// The factory invokes this to open external containers referenced by VPL `read` nodes.
/// It receives the resolved path (relative to `dir`), an optional SSH identity
/// for that one source, and returns a boxed reader.
///
/// `Send + Sync` because a [`PipelineFactory`] is shared across the worker
/// threads that build an operation graph. The bound belongs here rather than on
/// the factory: stated on the callback type, the compiler checks it for every
/// callback anyone installs.
type Callback =
	Box<dyn Fn(DataLocation, Option<PathBuf>) -> BoxFuture<'static, Result<Box<dyn TileSource>>> + Send + Sync>;

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
	dir: DataLocation,
	create_reader: Callback,
	runtime: TilesRuntime,
}

// The factory is shared while an operation graph is built, so it has to be
// thread-safe. Every field is, including the `Callback` — see the bound on its
// type above — so the compiler derives both traits and this assertion is what
// notices if that ever stops being true.
const _: fn() = || {
	fn assert_send_sync<T: Send + Sync>() {}
	assert_send_sync::<PipelineFactory>();
};

impl PipelineFactory {
	/// Creates an empty factory with no registered operations.
	#[must_use]
	pub fn new_empty(dir: DataLocation, create_reader: Callback, runtime: TilesRuntime) -> Self {
		PipelineFactory {
			read_ops: HashMap::new(),
			tran_ops: HashMap::new(),
			dir,
			create_reader,
			runtime,
		}
	}

	/// Creates a factory pre-loaded with all built-in read and transform operation factories.
	#[must_use]
	pub fn new_default(dir: DataLocation, create_reader: Callback, runtime: TilesRuntime) -> Self {
		let mut factory = PipelineFactory::new_empty(dir, create_reader, runtime);

		for f in read_operation_factories() {
			factory.add_read_factory(f);
		}

		for f in transform_operation_factories() {
			factory.add_tran_factory(f);
		}

		factory
	}

	/// Creates a factory that resolves readers using a built-in dummy callback.
	///
	/// Useful for examples and tests: resolves vector sources to `DummyVectorSource` and
	/// raster sources to `DummyImageSource` based on the filename’s extension/color code.
	#[must_use]
	pub fn new_dummy() -> Self {
		PipelineFactory::new_dummy_reader(Box::new(
			|location: DataLocation, _ssh_identity: Option<PathBuf>| -> BoxFuture<Result<Box<dyn TileSource>>> {
				Box::pin(async move {
					let mut name = location.to_string();
					let format = TileFormat::from_filename(&mut name)
						.ok_or_else(|| anyhow!("cannot determine tile format from filename '{location}'"))?;

					Ok(match format.to_type() {
						TileType::Vector => Box::new(DummyVectorSource::new(
							&[("dummy", &[&[("filename", &location.to_string())]])],
							None,
						)) as Box<dyn TileSource>,
						TileType::Raster => {
							let color = if !name.is_empty() && name.len() <= 4 {
								#[expect(clippy::cast_possible_truncation, reason = "a hex digit times 17 is 0..=255")]
								name
									.chars()
									.filter_map(|c| c.to_digit(16).map(|d| (d * 17) as u8))
									.collect()
							} else {
								vec![50, 150, 250]
							};
							Box::new(DummyImageSource::from_color(&color, 4, format, None)?) as Box<dyn TileSource>
						}
						_ => bail!("unsupported tile type for dummy reader in filename '{location}'"),
					})
				})
			},
		))
	}

	/// Creates a default-registered factory using the provided custom reader callback.
	#[must_use]
	pub fn new_dummy_reader(create_reader: Callback) -> Self {
		#[cfg(not(test))]
		let runtime = TilesRuntime::default();
		#[cfg(test)]
		let runtime = TilesRuntime::new_silent();

		PipelineFactory::new_default(DataLocation::from(PathBuf::new()), create_reader, runtime)
	}

	/// Creates a default-registered factory whose reads resolve through `runtime`.
	///
	/// This is the wiring [`PipelineReader::from_pipeline`](crate::PipelineReader::from_pipeline)
	/// uses, kept in one place. The callback [`new_default`](Self::new_default) takes is not
	/// something a caller outside this crate can reproduce by inspection: it has to resolve the
	/// [`DataLocation`] against the runtime, thread the per-source SSH identity through, and box
	/// the `Arc` the runtime returns rather than unwrap it — that last one is not a style choice,
	/// since unwrapping fails outright whenever the runtime already holds the reader in its cache.
	///
	/// So a caller driving the fold itself with
	/// [`read_operation_from_node`](Self::read_operation_from_node) — which is what
	/// [`PipelineReader::from_parts`](crate::PipelineReader::from_parts) does — gets the factory
	/// `PipelineReader` builds rather than a lookalike that drifts from it. See issue #262.
	#[must_use]
	pub fn new_runtime_reader(dir: impl Into<DataLocation>, runtime: TilesRuntime) -> Self {
		let callback = Self::runtime_reader_callback(runtime.clone());
		PipelineFactory::new_default(dir.into(), callback, runtime)
	}

	/// The reader callback [`new_runtime_reader`](Self::new_runtime_reader) installs.
	fn runtime_reader_callback(runtime: TilesRuntime) -> Callback {
		Box::new(
			move |location: DataLocation, ssh_identity: Option<PathBuf>| -> BoxFuture<Result<Box<dyn TileSource>>> {
				let runtime = runtime.clone();
				Box::pin(async move {
					let reader = runtime
						.reader_from_location_with_ssh_identity(location, ssh_identity.as_deref())
						.await?;
					// The runtime hands out a `SharedTileSource`, and the callback owes a `Box`.
					// Boxing the `Arc` satisfies that without claiming sole ownership of the
					// reader: nothing here mutates it, because `TileSource` has no `&mut self`
					// method to mutate it with. Unwrapping the `Arc` instead would fail outright
					// whenever the runtime already holds the reader in its cache. See issue #259.
					Ok(Box::new(reader) as Box<dyn TileSource>)
				})
			},
		)
	}

	/// Registers a read operation factory under its VPL tag name.
	fn add_read_factory(&mut self, factory: Box<dyn ReadOperationFactoryTrait>) {
		self.read_ops.insert(factory.tag_name().to_string(), factory);
	}

	/// Registers a transform operation factory under its VPL tag name.
	fn add_tran_factory(&mut self, factory: Box<dyn TransformOperationFactoryTrait>) {
		self.tran_ops.insert(factory.tag_name().to_string(), factory);
	}

	/// Invokes `create_reader` to open a container. Callers must resolve first.
	pub async fn reader(&self, location: DataLocation) -> Result<Box<dyn TileSource>> {
		self.reader_with_ssh_identity(location, None).await
	}

	/// As [`Self::reader`], with an SSH identity for this source alone.
	///
	/// A relative identity path is resolved against the VPL file's directory, the
	/// same base `filename` uses, so a pipeline that names both stays internally
	/// consistent.
	#[context("Failed to get reader for file '{}'", location)]
	pub async fn reader_with_ssh_identity(
		&self,
		location: DataLocation,
		ssh_identity: Option<&str>,
	) -> Result<Box<dyn TileSource>> {
		let location = location.resolved(&self.dir)?;
		let ssh_identity = match ssh_identity {
			Some(identity) => Some(
				DataLocation::from(PathBuf::from(identity))
					.resolved(&self.dir)?
					.to_path_buf()?,
			),
			None => None,
		};
		(self.create_reader.as_ref())(location, ssh_identity).await
	}

	/// Parses VPL text and builds the corresponding operation graph.
	#[context("Failed to create reader from VPL")]
	pub async fn operation_from_vpl(&self, text: &str) -> Result<Box<dyn TileSource>> {
		self.build_pipeline(parse_vpl(text)?).await
	}

	/// Parameter metadata for every registered read operation, by name.
	pub(crate) fn read_registry(&self) -> std::collections::HashMap<String, Vec<crate::vpl::VPLFieldMeta>> {
		self
			.read_ops
			.iter()
			.map(|(k, v)| (k.clone(), v.field_metadata()))
			.collect()
	}

	/// Parameter metadata for every registered transform operation, by name.
	pub(crate) fn transform_registry(&self) -> std::collections::HashMap<String, Vec<crate::vpl::VPLFieldMeta>> {
		self
			.tran_ops
			.iter()
			.map(|(k, v)| (k.clone(), v.field_metadata()))
			.collect()
	}

	/// `true` if `name` is a registered read operation.
	#[must_use]
	pub fn has_read_operation(&self, name: &str) -> bool {
		self.read_ops.contains_key(name)
	}

	/// `true` if `name` is a registered transform operation.
	#[must_use]
	pub fn has_transform_operation(&self, name: &str) -> bool {
		self.tran_ops.contains_key(name)
	}

	/// Parameter metadata for a read operation, or `None` if it is not registered.
	#[must_use]
	pub fn read_field_metadata(&self, name: &str) -> Option<Vec<crate::vpl::VPLFieldMeta>> {
		self.read_ops.get(name).map(|f| f.field_metadata())
	}

	/// Parameter metadata for a transform operation, or `None` if it is not registered.
	#[must_use]
	pub fn transform_field_metadata(&self, name: &str) -> Option<Vec<crate::vpl::VPLFieldMeta>> {
		self.tran_ops.get(name).map(|f| f.field_metadata())
	}

	/// Builds an executable operation graph from a parsed `VPLPipeline`.
	///
	/// Takes the head node as a read operation and folds the remaining nodes as transforms.
	#[context("Failed to build pipeline from VPL")]
	pub async fn build_pipeline(&self, pipeline: VPLPipeline) -> Result<Box<dyn TileSource>> {
		let pipeline_started = Instant::now();
		let (head, tail) = pipeline.split()?;
		log::trace!(
			"pipeline: building {} VPL node(s): head '{}' + {} transform(s)",
			1 + tail.len(),
			head.name,
			tail.len()
		);

		let head_name = head.name.clone();
		let node_started = Instant::now();
		let mut vpl_operation = self.read_operation_from_node(head).await?;
		log::trace!(
			"pipeline: read node '{head_name}' built in {:.2}s",
			node_started.elapsed().as_secs_f32()
		);

		for node in tail {
			let node_name = node.name.clone();
			let node_started = Instant::now();
			vpl_operation = self.tran_operation_from_node(node, vpl_operation).await?;
			log::trace!(
				"pipeline: transform node '{node_name}' built in {:.2}s",
				node_started.elapsed().as_secs_f32()
			);
		}

		log::trace!(
			"pipeline: built in {:.2}s total",
			pipeline_started.elapsed().as_secs_f32()
		);
		Ok(vpl_operation)
	}

	/// Instantiates a read operation from a VPL node using the registered factory.
	///
	/// This and [`tran_operation_from_node`](Self::tran_operation_from_node) are the two halves
	/// [`build_pipeline`](Self::build_pipeline) folds over, exposed so a caller can drive the fold
	/// itself. An editor rebuilding a pipeline after every keystroke wants to keep the read node it
	/// already built — that is where essentially all the cost is, since a transform only parses its
	/// parameters and wraps its input — and rebuild just the tail. Pull the nodes apart with
	/// [`VPLPipeline::split`](crate::VPLPipeline::split), keep the head as a
	/// [`SharedTileSource`](versatiles_container::SharedTileSource), and box a clone of it into the
	/// first transform. See issue #259.
	#[context("Failed to create read operation from VPL node")]
	pub async fn read_operation_from_node(&self, node: VPLNode) -> Result<Box<dyn TileSource>> {
		let factory = self
			.read_ops
			.get(&node.name)
			.ok_or_else(|| anyhow!("read operation '{}' unknown", node.name))?;

		factory.build(node, self).await
	}

	/// Instantiates a transform operation from a VPL node, wrapping `source`.
	///
	/// `source` is consumed, so a caller that wants to keep it — to build a different tail onto the
	/// same head — should hold it as a
	/// [`SharedTileSource`](versatiles_container::SharedTileSource) and pass `Box::new(shared.clone())`.
	/// See [`read_operation_from_node`](Self::read_operation_from_node).
	#[context("Failed to create transform operation from VPL node")]
	pub async fn tran_operation_from_node(
		&self,
		node: VPLNode,
		source: Box<dyn TileSource>,
	) -> Result<Box<dyn TileSource>> {
		let factory = self
			.tran_ops
			.get(&node.name)
			.ok_or_else(|| anyhow!("transform operation '{}' unknown", node.name))?;

		factory.build(node, source, self).await
	}

	/// Resolves a VPL-referenced location against `dir` and returns a new `DataLocation`.
	#[context("Failed to resolve location '{location:?}' against base dir")]
	pub fn resolve_location(&self, location: &DataLocation) -> Result<DataLocation> {
		location.resolved(&self.dir)
	}

	/// Returns rendered Markdown help listing all registered operations and their docs.
	///
	/// The `help.md` template may contain the placeholder `<!-- VPL_OPERATIONS_TOC -->`,
	/// which is replaced by an auto-generated table of contents linking to every
	/// operation, so the generated `README.md` carries an up-to-date operation index.
	pub fn help_md(&self) -> String {
		#[expect(
			clippy::borrowed_box,
			reason = "the slice comes from the registry, which stores boxed trait objects"
		)]
		fn to_md<T>(vec: &[&Box<T>]) -> String
		where
			T: OperationFactoryTrait + ?Sized,
		{
			vec.iter()
				.map(|f| format!("---\n\n## {}\n\n{}", f.tag_name(), f.docs()))
				.join("\n\n")
		}

		/// Inline list of links to each operation's section, e.g. ``[`filter`](#filter)``.
		#[expect(
			clippy::borrowed_box,
			reason = "the slice comes from the registry, which stores boxed trait objects"
		)]
		fn toc<T>(vec: &[&Box<T>]) -> String
		where
			T: OperationFactoryTrait + ?Sized,
		{
			vec.iter()
				.map(|f| format!("[`{name}`](#{name})", name = f.tag_name()))
				.join(" · ")
		}

		// Sort once; both the TOC and the detailed sections use the same order.
		let read_ops = self.read_ops.values().sorted_by_key(|f| f.tag_name()).collect_vec();
		let tran_ops = self.tran_ops.values().sorted_by_key(|f| f.tag_name()).collect_vec();

		let toc_section = format!(
			"## Operations\n\n**Read:** {}\n\n**Transform:** {}",
			toc(&read_ops),
			toc(&tran_ops),
		);

		let intro = include_str!("help.md").replace("<!-- VPL_OPERATIONS_TOC -->", &toc_section);

		let doc = [
			intro,
			String::from("---"),
			String::from("# READ operations"),
			to_md(&read_ops),
			String::from("---"),
			String::from("# TRANSFORM operations"),
			to_md(&tran_ops),
		]
		.join("\n\n");

		MULTIPLE_NEWLINES_REGEX.replace_all(&doc, "\n\n").to_string()
	}

	/// Returns the runtime associated with this factory.
	#[must_use]
	pub fn runtime(&self) -> TilesRuntime {
		self.runtime.clone()
	}
}

/// Metadata about a single VPL operation, used for code generation.
#[cfg(feature = "codegen")]
pub struct OperationMeta {
	/// The operation's name as written in VPL.
	pub tag_name: String,
	/// Whether this is a `"read"` or a `"transform"` operation.
	pub kind: &'static str,
	/// Full documentation: summary, details, and a rendered parameter list.
	///
	/// The parameter list duplicates `fields` in prose. Prefer `summary` and
	/// `details` for anything that is not rendering the complete reference.
	pub doc: String,
	/// First paragraph of `doc` — one sentence, for a tooltip or picker entry.
	pub summary: String,
	/// `doc` minus the summary and the parameter list. Empty when there is
	/// nothing more to say.
	pub details: String,
	/// One entry per argument, in declaration order.
	pub fields: Vec<crate::vpl::VPLFieldMeta>,
}

/// Returns metadata for all registered operations (both read and transform).
#[must_use]
#[cfg(feature = "codegen")]
pub fn all_operation_metadata() -> Vec<OperationMeta> {
	use crate::operations::{read_operation_factories, transform_operation_factories};

	let mut ops = Vec::new();

	for f in read_operation_factories() {
		ops.push(OperationMeta {
			tag_name: f.tag_name().to_string(),
			kind: "read",
			doc: f.docs(),
			summary: f.doc_summary(),
			details: f.doc_details(),
			fields: f.field_metadata(),
		});
	}

	for f in transform_operation_factories() {
		ops.push(OperationMeta {
			tag_name: f.tag_name().to_string(),
			kind: "transform",
			doc: f.docs(),
			summary: f.doc_summary(),
			details: f.doc_details(),
			fields: f.field_metadata(),
		});
	}

	ops
}

/// Every transform, paired with whether it could be applied to `source`.
///
/// The question a node picker actually asks — "what can go here" — so the loop
/// lives here rather than in every consumer.
///
/// Pairs rather than a map keyed by [`Compatibility`]: `Wrong` carries a reason
/// that differs per operation, so it cannot be a key. Ordered as
/// the crate registers them internally, which is stable across runs.
#[cfg(feature = "codegen")]
pub async fn compatible_transforms(source: &dyn TileSource) -> Vec<(OperationMeta, Compatibility)> {
	use crate::operations::transform_operation_factories;

	let mut out = Vec::new();
	for f in transform_operation_factories() {
		let meta = OperationMeta {
			tag_name: f.tag_name().to_string(),
			kind: "transform",
			doc: f.docs(),
			summary: f.doc_summary(),
			details: f.doc_details(),
			fields: f.field_metadata(),
		};
		out.push((meta, f.compatibility(source).await));
	}
	out
}

#[cfg(test)]
mod tests {
	use std::{path::Path, sync::Arc};

	use versatiles_container::SharedTileSource;

	use super::*;

	/// The point of exposing the two builders: a caller can keep the read node it already built
	/// and put a different tail on it, which is what makes an editor's rebuild cost proportional
	/// to the edit rather than to what the pipeline reads. See issue #259.
	#[tokio::test]
	async fn a_built_read_node_can_be_reused_for_several_tails() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let (head, tail) = parse_vpl("from_debug format=png | filter level_min=2 level_max=5")?.split()?;

		// Built once, and kept.
		let head: SharedTileSource = factory.read_operation_from_node(head).await?.into();

		// Two different tails onto the same head, neither of which consumes it.
		let filtered = factory
			.tran_operation_from_node(tail[0].clone(), Box::new(head.clone()))
			.await?;
		let bare = factory
			.tran_operation_from_node(parse_vpl("filter level_min=1")?.split()?.0, Box::new(head.clone()))
			.await?;

		assert_eq!(filtered.metadata().tile_format(), head.metadata().tile_format());
		assert_eq!(bare.metadata().tile_format(), head.metadata().tile_format());
		// the head is shared by both tails rather than rebuilt for either, and still usable
		assert_eq!(Arc::strong_count(&head), 3);
		assert!(head.tile_pyramid().await.is_ok());

		Ok(())
	}

	/// Folding the two builders by hand must produce what `build_pipeline` produces.
	#[tokio::test]
	async fn building_node_by_node_matches_build_pipeline() -> Result<()> {
		const VPL: &str = "from_debug format=png | filter level_min=2 level_max=5";
		let factory = PipelineFactory::new_dummy();

		let whole = factory.build_pipeline(parse_vpl(VPL)?).await?;

		let (head, tail) = parse_vpl(VPL)?.split()?;
		let mut folded = factory.read_operation_from_node(head).await?;
		for node in tail {
			folded = factory.tran_operation_from_node(node, folded).await?;
		}

		assert_eq!(folded.source_type(), whole.source_type());
		assert_eq!(folded.metadata(), whole.metadata());
		assert_eq!(folded.tilejson().stringify(), whole.tilejson().stringify());

		Ok(())
	}

	/// The callback [`PipelineFactory::new_runtime_reader`] installs is the one thing
	/// [`PipelineFactory::new_dummy`] cannot exercise — every other test here resolves reads to a
	/// synthetic source, so a break in the real wiring would show up first in someone else's
	/// repository. This opens an actual container through the runtime.
	#[tokio::test]
	async fn a_runtime_reader_factory_opens_a_real_container() -> Result<()> {
		let factory = PipelineFactory::new_runtime_reader(Path::new("../testdata"), TilesRuntime::new_silent());

		let operation = factory
			.operation_from_vpl("from_container filename=\"berlin.versatiles\"")
			.await?;

		let pyramid = operation.tile_pyramid().await?;
		assert_eq!(pyramid.level_min(), Some(0));
		assert_eq!(pyramid.level_max(), Some(14));

		Ok(())
	}

	/// The fold issue #262 exists to make reachable, driven through the runtime-backed callback
	/// rather than the dummy one: the read node is built once and two different tails go onto it,
	/// neither consuming it, and the folded result matches what `build_pipeline` produces.
	#[tokio::test]
	async fn a_runtime_read_node_can_be_reused_for_several_tails() -> Result<()> {
		const VPL: &str = "from_container filename=\"berlin.versatiles\" | filter level_min=10 level_max=12";
		let factory = PipelineFactory::new_runtime_reader(Path::new("../testdata"), TilesRuntime::new_silent());

		let whole = factory.build_pipeline(parse_vpl(VPL)?).await?;

		// Built once, and kept.
		let (head, tail) = parse_vpl(VPL)?.split()?;
		let head: SharedTileSource = factory.read_operation_from_node(head).await?.into();

		let mut folded: Box<dyn TileSource> = Box::new(head.clone());
		for node in tail {
			folded = factory.tran_operation_from_node(node, folded).await?;
		}
		let other = factory
			.tran_operation_from_node(parse_vpl("filter level_min=11")?.split()?.0, Box::new(head.clone()))
			.await?;

		assert_eq!(folded.source_type(), whole.source_type());
		assert_eq!(folded.metadata(), whole.metadata());
		assert_eq!(folded.tilejson().stringify(), whole.tilejson().stringify());

		// the two tails are different pipelines over the same head, not the same one twice
		let folded_pyramid = folded.tile_pyramid().await?;
		let other_pyramid = other.tile_pyramid().await?;
		assert_eq!(
			(folded_pyramid.level_min(), folded_pyramid.level_max()),
			(Some(10), Some(12))
		);
		assert_eq!(
			(other_pyramid.level_min(), other_pyramid.level_max()),
			(Some(11), Some(14))
		);

		// the head is shared by both tails rather than read again for either, and still usable
		assert_eq!(Arc::strong_count(&head), 3);
		assert!(!head.tile_pyramid().await?.is_empty());

		Ok(())
	}

	/// Every operation this build registers must have a section in the generated
	/// reference, and every section must name an operation that exists.
	///
	/// `versatiles_pipeline/README.md` is generated by `scripts/build-docs-readme.sh`
	/// from the binary, so it is correct the moment it is regenerated and drifts
	/// silently from then on: an operation added, renamed or removed without
	/// rerunning the script leaves the reference describing a program that no
	/// longer exists. This asserts the two sets agree, without depending on the
	/// script having been run.
	#[test]
	fn every_operation_appears_in_the_generated_reference() {
		use crate::operations::{read_operation_factories, transform_operation_factories};

		let reference = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
			.expect("the generated reference should be readable");

		let documented: std::collections::HashSet<&str> = reference
			.lines()
			.filter_map(|line| line.strip_prefix("## "))
			.map(str::trim)
			.collect();

		let registered: Vec<String> = read_operation_factories()
			.iter()
			.map(|f| f.tag_name().to_string())
			.chain(transform_operation_factories().iter().map(|f| f.tag_name().to_string()))
			.collect();

		let undocumented: Vec<&String> = registered
			.iter()
			.filter(|name| !documented.contains(name.as_str()))
			.collect();
		assert!(
			undocumented.is_empty(),
			"operations missing from versatiles_pipeline/README.md — regenerate it with \
			 scripts/build-docs-readme.sh:\n  {undocumented:?}"
		);

		// The other direction only holds when every feature-gated operation is
		// compiled in; the reference is generated with `--features gdal`.
		#[cfg(feature = "gdal")]
		{
			let registered: std::collections::HashSet<&str> = registered.iter().map(String::as_str).collect();
			// Section headings that are prose rather than operations.
			const PROSE: &[&str] = &[
				"Operations",
				"Defining a pipeline",
				"Operation Format",
				"Filter expressions (CEL)",
			];
			let stale: Vec<&&str> = documented
				.iter()
				.filter(|name| !registered.contains(**name) && !PROSE.contains(*name))
				.collect();
			assert!(
				stale.is_empty(),
				"versatiles_pipeline/README.md documents operations that no longer exist:\n  {stale:?}"
			);
		}
	}
	use crate::helpers::{dummy_image_source::DummyImageSource, dummy_vector_source::DummyVectorSource};

	fn raster_source() -> Box<dyn TileSource> {
		Box::new(DummyImageSource::new(|_| None, TileFormat::PNG, None).unwrap())
	}

	fn vector_source() -> Box<dyn TileSource> {
		Box::new(DummyVectorSource::new(&[("places", &[&[("name", "Berlin")]])], None))
	}

	/// Every transform, by tag, with its verdict against one source.
	async fn verdicts(source: &dyn TileSource) -> Vec<(String, Compatibility)> {
		let mut out = Vec::new();
		for f in crate::operations::transform_operation_factories() {
			out.push((f.tag_name().to_string(), f.compatibility(source).await));
		}
		out
	}

	fn tags_that_fit(verdicts: &[(String, Compatibility)]) -> Vec<&str> {
		verdicts
			.iter()
			.filter(|(_, c)| c.fits())
			.map(|(tag, _)| tag.as_str())
			.collect()
	}

	#[tokio::test]
	async fn raster_source_offers_raster_and_dem_but_not_vector() {
		let source = raster_source();
		let verdicts = verdicts(&*source).await;
		let fits = tags_that_fit(&verdicts);

		// dem_* stays on the list: nothing separates terrarium elevation from a
		// photograph, so it cannot be refused.
		for tag in ["raster_flatten", "raster_overview", "dem_quantize", "dem_overview"] {
			assert!(fits.contains(&tag), "{tag} should fit a PNG source, got {fits:?}");
		}
		for tag in ["vector_repair", "vector_filter_layers", "vector_overzoom"] {
			assert!(!fits.contains(&tag), "{tag} should not fit a PNG source");
		}
	}

	#[tokio::test]
	async fn vector_source_offers_vector_but_not_raster() {
		let source = vector_source();
		let verdicts = verdicts(&*source).await;
		let fits = tags_that_fit(&verdicts);

		for tag in [
			"vector_repair",
			"vector_filter_layers",
			"vector_update_properties",
			"vector_overzoom",
		] {
			assert!(fits.contains(&tag), "{tag} should fit an MVT source, got {fits:?}");
		}
		for tag in ["raster_flatten", "raster_overview", "dem_quantize"] {
			assert!(!fits.contains(&tag), "{tag} should not fit an MVT source");
		}
	}

	#[tokio::test]
	async fn format_agnostic_transforms_fit_everything() {
		for source in [raster_source(), vector_source()] {
			let verdicts = verdicts(&*source).await;
			let fits = tags_that_fit(&verdicts);
			for tag in ["filter", "meta_update", "remap_coords"] {
				assert!(fits.contains(&tag), "{tag} should fit any source, got {fits:?}");
			}
		}
	}

	#[tokio::test]
	async fn a_refusal_always_carries_a_usable_reason() {
		// The whole point of putting the reason on the variant: a UI can never
		// receive a `Wrong` it has nothing to display.
		let source = raster_source();
		for (tag, verdict) in verdicts(&*source).await {
			let Compatibility::Wrong(reason) = &verdict else {
				continue;
			};
			assert!(!reason.is_empty(), "{tag} refused with an empty reason");
			assert!(
				reason.contains(&tag),
				"{tag}: reason should name the operation: {reason}"
			);
			assert!(
				reason.contains("png"),
				"{tag}: reason should name the source format: {reason}"
			);
		}
	}

	#[tokio::test]
	async fn a_fitting_transform_can_actually_be_built() {
		// The direction that must never break: hiding an operation a picker
		// should have offered is the failure the default `Fits` exists to avoid.
		let factory = PipelineFactory::new_dummy();

		for (tag, make) in [
			("raster_flatten", raster_source as fn() -> Box<dyn TileSource>),
			("vector_repair", vector_source as fn() -> Box<dyn TileSource>),
		] {
			let find = || {
				crate::operations::transform_operation_factories()
					.into_iter()
					.find(|f| f.tag_name() == tag)
					.expect("registered transform")
			};
			assert!(find().compatibility(&*make()).await.fits(), "{tag} should fit");
			assert!(
				find()
					.build(crate::vpl::VPLNode::from(tag), make(), &factory)
					.await
					.is_ok(),
				"{tag} said it fits but would not build"
			);
		}
	}

	#[tokio::test]
	async fn every_refused_transform_also_fails_to_build() {
		// `build` consults `compatibility()` first, so the two can no longer
		// disagree. Before that, `raster_flatten` on an MVT source built happily
		// and only failed once a tile was pulled through it — at which point the
		// per-tile error had no way left to be an `Err` and became a panic.
		//
		// The check runs before the node's arguments are read, which is why a
		// bare `VPLNode::from(tag)` is enough here even for operations with
		// required parameters: the refusal must come first, or the caller is told
		// about a missing argument for an operation they could never have used.
		let factory = PipelineFactory::new_dummy();

		for (make_source, label) in [
			(raster_source as fn() -> Box<dyn TileSource>, "png"),
			(vector_source as fn() -> Box<dyn TileSource>, "mvt"),
		] {
			for f in crate::operations::transform_operation_factories() {
				let Compatibility::Wrong(reason) = f.compatibility(&*make_source()).await else {
					continue;
				};
				let tag = f.tag_name().to_string();
				let error = f
					.build(crate::vpl::VPLNode::from(tag.as_str()), make_source(), &factory)
					.await
					.expect_err(&format!("{tag} refused a {label} source but still built one"));

				assert_eq!(
					format!("{error:#}"),
					reason,
					"{tag}: the build should fail with the reason the verdict gave"
				);
			}
		}
	}

	#[cfg(feature = "codegen")]
	#[tokio::test]
	async fn compatible_transforms_pairs_every_transform_with_a_verdict() {
		let source = vector_source();
		let pairs = compatible_transforms(&*source).await;

		assert_eq!(pairs.len(), crate::operations::transform_operation_factories().len());
		for (meta, _) in &pairs {
			assert_eq!(meta.kind, "transform");
			assert!(!meta.summary.is_empty(), "{} has no summary", meta.tag_name);
		}
		assert!(pairs.iter().any(|(_, c)| c.fits()));
		assert!(pairs.iter().any(|(_, c)| !c.fits()));
	}

	#[test]
	fn help_md_includes_operations_toc() {
		let md = PipelineFactory::new_dummy().help_md();

		// The help.md placeholder must be replaced, not emitted verbatim.
		assert!(!md.contains("VPL_OPERATIONS_TOC"), "TOC placeholder was not replaced");

		// TOC heading and the two operation groups.
		assert!(md.contains("## Operations"), "missing TOC heading:\n{md}");
		assert!(md.contains("**Read:**"), "missing read group:\n{md}");
		assert!(md.contains("**Transform:**"), "missing transform group:\n{md}");

		// Each TOC entry links to the section anchor that actually exists below.
		assert!(
			md.contains("[`from_container`](#from_container)"),
			"missing read op link:\n{md}"
		);
		assert!(md.contains("[`filter`](#filter)"), "missing transform op link:\n{md}");
		assert!(md.contains("## from_container"), "missing op section heading:\n{md}");

		// The TOC comes before the detailed operation sections.
		let toc_pos = md.find("## Operations").expect("TOC heading present");
		let read_section = md.find("# READ operations").expect("READ section present");
		assert!(toc_pos < read_section, "TOC should appear before the detailed sections");
	}
}
