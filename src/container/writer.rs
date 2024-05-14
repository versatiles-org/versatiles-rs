use super::TilesReader;
use crate::types::{DataWriterFile, DataWriterTrait};
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

#[async_trait]
pub trait TilesWriter: Send {
	// readers must be mutable, because they might use caching
	async fn write_to_path(reader: &mut dyn TilesReader, path: &Path) -> Result<()> {
		Self::write_to_writer(reader, &mut DataWriterFile::from_path(path)?).await
	}
	async fn write_to_writer(reader: &mut dyn TilesReader, writer: &mut dyn DataWriterTrait) -> Result<()>;
}
