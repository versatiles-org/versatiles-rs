use crate::{
	containers::{TilesConverterBox, TilesConverterTrait, TilesReaderBox},
	shared::TilesConverterConfig,
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use std::path::Path;

pub struct MBTilesConverter;

#[async_trait]
impl TilesConverterTrait for MBTilesConverter {
	async fn open_file(_path: &Path, _config: TilesConverterConfig) -> Result<TilesConverterBox>
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
	use super::MBTilesConverter;
	use crate::{containers::TilesConverterTrait, shared::TilesConverterConfig};
	use std::path::Path;

	#[tokio::test]
	async fn panic1() {
		assert!(
			MBTilesConverter::open_file(&Path::new("filename.txt"), TilesConverterConfig::new_full())
				.await
				.is_err()
		);
	}
}
