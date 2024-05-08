use super::TilesReaderBox;
use crate::shared::{Compression, TileFormat};
use anyhow::{ensure, Result};
use async_trait::async_trait;

pub type TilesWriterBox = Box<dyn TilesWriterTrait>;

#[derive(Debug)]
pub struct TilesWriterParameters {
	pub tile_format: TileFormat,
	pub tile_compression: Compression,
}
impl TilesWriterParameters {
	#[allow(dead_code)]
	pub fn new(tile_format: TileFormat, tile_compression: Compression) -> TilesWriterParameters {
		TilesWriterParameters {
			tile_format,
			tile_compression,
		}
	}
}

#[allow(clippy::new_ret_no_self)]
#[async_trait]
pub trait TilesWriterTrait: Send {
	// readers must be mutable, because they might use caching
	async fn write_from_reader(&mut self, reader: &mut TilesReaderBox) -> Result<()> {
		self.check_reader(reader)?;
		self.write_tiles(reader).await?;
		return Ok(());
	}

	// readers must be mutable, because they might use caching
	fn check_reader(&self, reader: &mut TilesReaderBox) -> Result<()> {
		let reader_parameters = reader.get_parameters();
		let writer_parameters = self.get_parameters();

		ensure!(
			reader_parameters.tile_format == writer_parameters.tile_format,
			"tile format must be the same"
		);

		ensure!(
			reader_parameters.tile_compression == writer_parameters.tile_compression,
			"tile compression must be the same"
		);

		Ok(())
	}

	fn get_parameters(&self) -> &TilesWriterParameters;
	async fn write_tiles(&mut self, reader: &mut TilesReaderBox) -> Result<()>;
}
