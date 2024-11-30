//! Module `writer` provides traits and implementations for writing tiles to various container formats.
//!
//! The `TilesWriter` trait defines the necessary methods to be implemented by any tile writer.
//! It includes methods for writing tile data from a `TilesReader` to a specified path or writer.
//!

use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use versatiles_core::{types::TilesReaderTrait, utils::io::*};

/// Trait defining the behavior of a tile writer.
#[async_trait]
pub trait TilesWriterTrait: Send {
	/// Write tile data from a reader to a specified path.
	async fn write_to_path(reader: &mut dyn TilesReaderTrait, path: &Path) -> Result<()> {
		Self::write_to_writer(reader, &mut DataWriterFile::from_path(path)?).await
	}

	/// Write tile data from a reader to a writer.
	async fn write_to_writer(
		reader: &mut dyn TilesReaderTrait,
		writer: &mut dyn DataWriterTrait,
	) -> Result<()>;
}
