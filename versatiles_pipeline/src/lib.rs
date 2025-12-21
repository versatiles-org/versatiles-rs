//! VersaTiles Pipeline Engine
//!
//! This crate implements the VersaTiles **pipeline engine**, which builds and executes tile processing graphs defined in the **VersaTiles Pipeline Language (VPL)**.
//!
//! Pipelines consist of **read operations** (data sources) and **transform operations** (data processors), which are connected dynamically at runtime.
//!
//! The main entry points are [`PipelineFactory`] (for building operation graphs from VPL) and [`PipelineReader`] (for executing them via the container interface).
//!
//! This crate integrates tightly with [`versatiles_container`] and [`versatiles_core`] for tile I/O and metadata management.

mod factory;
mod helpers;
mod operations;
mod traits;
mod vpl;

pub use factory::PipelineFactory;
pub use helpers::{PipelineReader, register_pipeline_readers};
pub use traits::OperationTrait;
pub use vpl::VPLNode;
