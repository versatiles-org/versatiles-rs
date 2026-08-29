//! VersaTiles Pipeline Engine
//!
//! This crate implements the VersaTiles **pipeline engine**, which builds and executes tile processing graphs defined in the **VersaTiles Pipeline Language (VPL)**.
//!
//! Pipelines consist of **read operations** (data sources) and **transform operations** (data processors), which are connected dynamically at runtime.
//!
//! The main entry points are [`PipelineFactory`] (for building operation graphs from VPL) and [`PipelineReader`] (for executing them via the container interface).
//!
//! A consumer that rebuilds pipelines repeatedly — an editor watching a map redraw as someone
//! types — should reach for [`PipelineFactory::new_runtime_reader`] and
//! [`PipelineReader::from_parts`] instead of rebuilding the whole graph. Building a transform
//! costs a parameter parse; building a read node costs whatever it takes to load the source,
//! seconds for a large CSV or GeoJSON. Both of those let a caller keep the read node it already
//! built across an edit to a transform. See issues #259 and #262.
//!
//! [`CsvReader`] is exported too: it is the reader `from_csv` uses, so a consumer that
//! needs to inspect a CSV before building a pipeline sees exactly what the pipeline will.
//!
//! The operation factory traits — [`OperationFactoryTrait`], [`ReadOperationFactoryTrait`]
//! and [`TransformOperationFactoryTrait`] — are public so a consumer can ask an operation
//! about itself: its docs, its parameters, and via
//! [`TransformOperationFactoryTrait::compatibility`] whether it applies to a given source.
//!
//! This crate integrates tightly with [`versatiles_container`] and [`versatiles_core`] for tile I/O and metadata management.

mod check;
pub use check::{VplProblem, check_pipeline};

mod factory;
mod helpers;
mod operations;
pub mod vpl;

pub use factory::{
	Compatibility, OperationFactoryTrait, PipelineFactory, ReadOperationFactoryTrait, TransformOperationFactoryTrait,
	require_tile_type,
};
#[cfg(feature = "codegen")]
pub use factory::{OperationMeta, all_operation_metadata, compatible_transforms};
pub use helpers::{CsvReader, PipelineReader, register_pipeline_readers};
pub use vpl::{VPLNode, VPLPipeline};
