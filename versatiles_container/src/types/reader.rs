//! Defines the interface for reading tile data from various container formats.
//!
//! This module provides the [`TilesReader`] trait, which enables opening tile containers
//! from filesystem paths or generic [`DataReader`] sources.
//!
//! Implementations of this trait are registered in the [`ContainerRegistry`] to handle specific
//! input formats (e.g. `.mbtiles`, `.pmtiles`, `.versatiles`, `.tar`).

use crate::{SharedTileSource, TilesRuntime};
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::path::Path;
use versatiles_core::io::{DataReader, DataReaderFile};

/// Interface for opening tile containers from paths or data readers.
///
/// Readers implement deserialization from a specific format (e.g., `MBTiles`, `VersaTiles`, TAR),
/// and can operate either on filesystem paths or any source implementing [`DataReader`].
///
/// Implementors that support generic data readers get a default `open_path` that delegates
/// via [`DataReaderFile`]. File-only readers override `open_path` and set
/// `supports_data_reader() -> false`.
#[async_trait]
pub trait TilesReader: Send {
	/// Returns `true` when the reader can open from a generic [`DataReader`] source
	/// (e.g. for HTTP). File-only readers (MBTiles, TAR) return `false`.
	#[must_use]
	fn supports_data_reader() -> bool {
		true
	}

	/// Opens a tile container from the file or directory at `path`.
	///
	/// The default implementation wraps `path` in a [`DataReaderFile`] and calls
	/// [`TilesReader::open_reader`]. Implementations may override this for formats
	/// that require direct filesystem access.
	///
	/// # Errors
	/// Returns an error if the file cannot be opened or the container is invalid.
	async fn open_path(path: &Path, runtime: TilesRuntime) -> Result<SharedTileSource> {
		Self::open_reader(DataReaderFile::open(path)?, runtime).await
	}

	/// Opens a tile container from the provided [`DataReader`] source.
	///
	/// The default implementation bails with "not supported". Readers that support generic
	/// data sources should override this method.
	///
	/// # Errors
	/// Returns an error if the format does not support generic readers, or if I/O fails.
	async fn open_reader(_reader: DataReader, _runtime: TilesRuntime) -> Result<SharedTileSource> {
		bail!("this format does not support reading from a generic data reader")
	}
}
