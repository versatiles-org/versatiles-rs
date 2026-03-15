//! Defines the interface for writing tile data to various container formats.
//!
//! This module provides the object‑safe [`TilesWriter`], which enables writing tiles
//! from any [`TileSource`] source into a file or arbitrary output writer implementing
//! [`DataWriterTrait`].
//!
//! Implementations of this trait are registered in the [`ContainerRegistry`] to handle specific
//! output formats (e.g. `.mbtiles`, `.pmtiles`, `.versatiles`, `.tar`, or directory trees).
//!
//! ## Responsibilities
//! A tile writer must:
//! - Pull tiles from a [`TileSource`] source (possibly streamed)
//! - Serialize them to the target format
//! - Respect [`TilesRuntime`] parameters such as compression and parallelism
//!
//! ## Example
//! ```rust
//! use versatiles_container::*;
//! use versatiles_core::*;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let runtime = TilesRuntime::default();
//!     let reader = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;
//!     let output_path = std::env::temp_dir().join("example.versatiles");
//!
//!     // The runtime automatically dispatches to the correct writer
//!     runtime.write_to_path(reader, &output_path).await?;
//!     Ok(())
//! }
//! ```

use crate::{TileSource, TilesRuntime};
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::path::Path;
use versatiles_core::io::{DataWriterFile, DataWriterTrait};

/// Object‑safe interface for writing tiles from a reader into a container format.
///
/// Writers implement serialization to a specific format (e.g., `MBTiles`, `VersaTiles`, TAR),
/// and can operate either on filesystem paths or any sink implementing [`DataWriterTrait`].
///
/// Implementors should handle compression, metadata, and configuration from [`TilesRuntime`].
#[async_trait]
pub trait TilesWriter: Send {
	/// Returns `true` when the writer can serialize to a generic [`DataWriterTrait`] sink
	/// (e.g. for SFTP output). File-only writers (MBTiles, Directory) return `false`.
	#[must_use]
	fn supports_data_writer() -> bool {
		true
	}

	/// Writes all tile data from `reader` into the file or directory at `path`.
	///
	/// The default implementation wraps `path` in a [`DataWriterFile`] and calls
	/// [`TilesWriter::write_to_writer`]. Implementations may override this for more efficient
	/// file handling.
	///
	/// # Errors
	/// Returns an error if the file cannot be created or the writing operation fails.
	async fn write_to_path(reader: &mut dyn TileSource, path: &Path, runtime: TilesRuntime) -> Result<()> {
		Self::write_to_writer(reader, &mut DataWriterFile::from_path(path)?, runtime).await
	}

	/// Writes tile data from `reader` to the provided [`DataWriterTrait`] sink.
	///
	/// The default implementation bails with "not supported". Writers that support generic
	/// output sinks should override this method.
	///
	/// # Errors
	/// Returns an error if the format does not support generic writers, or if I/O fails.
	async fn write_to_writer(
		_reader: &mut dyn TileSource,
		_writer: &mut dyn DataWriterTrait,
		_runtime: TilesRuntime,
	) -> Result<()> {
		bail!("this format does not support writing to a generic data writer")
	}
}
