//! Defines the interface for writing tile data to various container formats.
//!
//! This module provides the object‑safe [`TilesWriterTrait`], which enables writing tiles
//! from any [`TilesReaderTrait`] source into a file or arbitrary output writer implementing
//! [`DataWriterTrait`].
//!
//! Implementations of this trait are registered in the [`ContainerRegistry`] to handle specific
//! output formats (e.g. `.mbtiles`, `.pmtiles`, `.versatiles`, `.tar`, or directory trees).
//!
//! ## Responsibilities
//! A tile writer must:
//! - Pull tiles from a [`TilesReaderTrait`] source (possibly streamed)
//! - Serialize them to the target format
//! - Respect [`ProcessingConfig`] parameters such as compression and parallelism
//!
//! ## Example
//! ```rust
//! use versatiles_container::*;
//! use versatiles_core::*;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let registry = ContainerRegistry::default();
//!     let reader = registry.get_reader("../testdata/berlin.mbtiles").await?;
//!     let output_path = std::env::temp_dir().join("example.versatiles");
//!
//!     // The registry automatically dispatches to the correct writer
//!     registry.write_to_path(reader, &output_path).await?;
//!     Ok(())
//! }
//! ```

use crate::{ProcessingConfig, TilesReaderTrait};
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use versatiles_core::io::*;

/// Object‑safe interface for writing tiles from a reader into a container format.
///
/// Writers implement serialization to a specific format (e.g., MBTiles, VersaTiles, TAR),
/// and can operate either on filesystem paths or any sink implementing [`DataWriterTrait`].
///
/// Implementors should handle compression, metadata, and configuration from [`ProcessingConfig`].
#[async_trait]
pub trait TilesWriterTrait: Send {
	/// Writes all tile data from `reader` into the file or directory at `path`.
	///
	/// The default implementation wraps `path` in a [`DataWriterFile`] and calls
	/// [`write_to_writer`]. Implementations may override this for more efficient
	/// file handling.
	///
	/// # Errors
	/// Returns an error if the file cannot be created or the writing operation fails.
	async fn write_to_path(reader: &mut dyn TilesReaderTrait, path: &Path, config: ProcessingConfig) -> Result<()> {
		Self::write_to_writer(reader, &mut DataWriterFile::from_path(path)?, config).await
	}

	/// Writes tile data from `reader` to the provided [`DataWriterTrait`] sink.
	///
	/// Implementations must serialize tiles according to their format and use the
	/// [`ProcessingConfig`] to control parallelism, buffering, or compression.
	///
	/// # Arguments
	/// - `reader`: Source tile reader providing tile data.
	/// - `writer`: Output sink implementing [`DataWriterTrait`].
	/// - `config`: Writer configuration (compression, parallelism, etc.).
	///
	/// # Errors
	/// Returns an error if reading from the source or writing to the sink fails.
	async fn write_to_writer(
		reader: &mut dyn TilesReaderTrait,
		writer: &mut dyn DataWriterTrait,
		config: ProcessingConfig,
	) -> Result<()>;
}
