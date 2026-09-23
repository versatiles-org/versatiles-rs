//! Defines the interface for writing tile data to various container formats.
//!
//! This module provides the object‑safe [`TilesWriter`], which enables writing tiles
//! from any [`TileSource`] source into a file or arbitrary output writer implementing
//! [`DataWriterTrait`].
//!
//! Implementations of this trait are registered in the [`ContainerRegistry`](crate::ContainerRegistry) to handle specific
//! output formats (e.g. `.mbtiles`, `.pmtiles`, `.versatiles`, `.tar`, or directory trees).
//!
//! ## Responsibilities
//! A tile writer must:
//! - Pull tiles from a [`TileSource`] source (possibly streamed)
//! - Serialize them to the target format
//! - Respect [`TilesRuntime`] parameters such as compression and parallelism
//!
//! ## What a writer must *not* do
//! A writer never decides where its output ends up. It is handed a path that
//! does not exist, writes a complete container there, and touches nothing else:
//! no deleting an existing destination, no renaming, no cleaning up after
//! itself when it fails. See [`TilesWriter::write_to_path`] for why that
//! division exists and what the caller does on either side of it.
//!
//! ## Example
//! ```rust
//! use versatiles_container::*;
//! use versatiles_core::*;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let runtime = TilesRuntime::default();
//!     let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
//!     let output_path = std::env::temp_dir().join("example_writer.versatiles");
//!
//!     // The runtime automatically dispatches to the correct writer
//!     runtime.write_to_path(reader, &output_path).await?;
//!     Ok(())
//! }
//! ```

use std::path::Path;

use anyhow::{Result, bail};
use async_trait::async_trait;
use versatiles_core::io::{DataWriterFile, DataWriterTrait};

use crate::{TileSource, TilesRuntime};

/// Object‑safe interface for writing tiles from a reader into a container format.
///
/// Writers implement serialization to a specific format (e.g., `MBTiles`, `VersaTiles`, TAR),
/// and can operate either on filesystem paths or any sink implementing [`DataWriterTrait`].
///
/// Implementors should handle compression, metadata, and configuration from [`TilesRuntime`].
///
/// Writes a source out in one pass; [`TileSink`](crate::TileSink) is the
/// incremental counterpart. See the [crate documentation](crate) for how the
/// four IO traits relate.
#[async_trait]
pub trait TilesWriter: Send {
	/// Returns `true` when the writer can serialize to a generic [`DataWriterTrait`] sink
	/// (e.g. for SFTP output). File-only writers (MBTiles, Directory) return `false`.
	#[must_use]
	fn supports_data_writer() -> bool {
		true
	}

	/// Format-specific option keys this writer understands, in the order a help
	/// message should list them.
	///
	/// Writers take no arguments of their own, so options travel on
	/// [`TilesRuntime`]. Declaring them here lets the registry reject a key no
	/// writer will read *before* anything is opened — otherwise `-w
	/// allow_unclusterd=true` would be accepted and silently do nothing, which
	/// is the failure mode writer options exist to prevent.
	///
	/// Defaults to none, so a writer opts in by listing its keys.
	#[must_use]
	fn supported_options() -> &'static [&'static str] {
		&[]
	}

	/// Writes all tile data from `reader` into the file or directory at `path`.
	///
	/// # The contract
	///
	/// **`path` is not the destination.** It is a staging location that does not
	/// exist yet, beside the destination the user asked for. The caller
	/// ([`ContainerRegistry::write_to_path`](crate::ContainerRegistry::write_to_path))
	/// moves the result into place afterwards. So an implementation:
	///
	/// - **creates `path` and writes a complete container there.** On returning
	///   `Ok`, everything the format needs — header, index, terminator — must be
	///   on disk.
	/// - **touches nothing else.** In particular it must never delete or truncate
	///   the user's destination: it does not know which path that is, and that is
	///   deliberate.
	/// - **does not clean up after a failure.** Returning `Err` is enough; the
	///   caller removes the staging location. A writer that tidied up would be
	///   racing the caller for the same file.
	///
	/// # Why
	///
	/// Every writer used to decide this for itself, and the decisions disagreed —
	/// one truncated the destination, one deleted it outright before opening a
	/// database, and a later layer moved whatever it found there out of the way.
	/// Two layers acting on one run is how a finished conversion came to be
	/// replaced by an unfinished one. Keeping the decision in exactly one place is
	/// what stops that, and this is the half of the bargain a writer keeps.
	///
	/// The property the caller can then guarantee: a destination only ever holds a
	/// complete output. A conversion that fails part-way leaves whatever was there
	/// before exactly as it was.
	///
	/// # Default
	///
	/// Wraps `path` in a [`DataWriterFile`], calls
	/// [`write_to_writer`](TilesWriter::write_to_writer), and finalizes it.
	/// Override for more efficient file handling — but keep the contract above.
	///
	/// # Errors
	/// Returns an error if the file cannot be created or the writing operation fails.
	async fn write_to_path(reader: &dyn TileSource, path: &Path, runtime: TilesRuntime) -> Result<()> {
		let mut writer = DataWriterFile::from_path(path)?;
		Self::write_to_writer(reader, &mut writer, runtime).await?;
		// Without this the buffer reaches the disk only via `BufWriter`'s
		// destructor, which cannot report a failure — so a disk filling up on the
		// last flush ended the conversion successfully with a truncated file.
		writer.finalize().await
	}

	/// Writes tile data from `reader` to the provided [`DataWriterTrait`] sink.
	///
	/// The default implementation bails with "not supported". Writers that support generic
	/// output sinks should override this method.
	///
	/// # Errors
	/// Returns an error if the format does not support generic writers, or if I/O fails.
	async fn write_to_writer(
		_reader: &dyn TileSource,
		_writer: &mut dyn DataWriterTrait,
		_runtime: TilesRuntime,
	) -> Result<()> {
		bail!("this format does not support writing to a generic data writer")
	}
}
