use crate::{
	containers::{TilesConverterBox, TilesConverterTrait, TilesReaderBox},
	shared::TilesConverterConfig,
};
use anyhow::{bail, Result};
use async_trait::async_trait;

pub struct MBTilesConverter;

#[async_trait]
impl TilesConverterTrait for MBTilesConverter {
	async fn new(_filename: &str, _config: TilesConverterConfig) -> Result<TilesConverterBox>
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
	use crate::{
		containers::{mock, TilesConverterTrait, TilesReaderTrait},
		shared::TilesConverterConfig,
	};

	#[tokio::test]
	async fn panic1() {
		assert!(MBTilesConverter::new("filename.txt", TilesConverterConfig::new_full())
			.await
			.is_err());
	}

	#[tokio::test]
	#[should_panic]
	async fn panic2() {
		let mut converter = MBTilesConverter {};
		let mut reader = mock::MockTilesReader::new("filename.txt").await.unwrap();
		assert!(converter.convert_from(&mut reader).await.is_err())
	}
}
