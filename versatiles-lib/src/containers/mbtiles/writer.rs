use crate::{
	containers::{TilesReaderBox, TilesWriterBox, TilesWriterTrait},
	shared::TilesWriterConfig,
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use std::path::Path;

pub struct MBTilesWriter;

#[async_trait]
impl TilesWriterTrait for MBTilesWriter {
	async fn open_file(_path: &Path, _config: TilesWriterConfig) -> Result<TilesWriterBox>
	where
		Self: Sized,
	{
		bail!("conversion to mbtiles is not supported")
	}
	async fn convert_from(&mut self, _reader: &mut TilesReaderBox) -> Result<()> {
		bail!("conversion to mbtiles is not supported")
	}
}

#[cfg(test)]
mod tests {
	use super::MBTilesWriter;
	use crate::{containers::TilesWriterTrait, shared::TilesWriterConfig};
	use std::path::Path;

	#[tokio::test]
	async fn panic1() {
		assert!(
			MBTilesWriter::open_file(&Path::new("filename.txt"), TilesWriterConfig::new_full())
				.await
				.is_err()
		);
	}
}
