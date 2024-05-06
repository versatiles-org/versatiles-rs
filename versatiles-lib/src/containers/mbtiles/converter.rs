use crate::{
	containers::{TileConverterBox, TileConverterTrait, TileReaderBox},
	shared::TileConverterConfig,
};
use anyhow::{bail, Result};
use async_trait::async_trait;

pub struct TileConverter;

#[async_trait]
impl TileConverterTrait for TileConverter {
	async fn new(_filename: &str, _config: TileConverterConfig) -> Result<TileConverterBox>
	where
		Self: Sized,
	{
		bail!("conversion to mbtiles is not supported")
	}
	async fn convert_from(&mut self, _reader: &mut TileReaderBox) -> Result<()> {
		bail!("conversion to mbtiles is not supported")
	}
}

#[cfg(test)]
mod tests {
	use super::TileConverter;
	use crate::{
		containers::{mock, TileConverterTrait, TileReaderTrait},
		shared::TileConverterConfig,
	};

	#[tokio::test]
	async fn panic1() {
		assert!(TileConverter::new("filename.txt", TileConverterConfig::new_full())
			.await
			.is_err());
	}

	#[tokio::test]
	#[should_panic]
	async fn panic2() {
		let mut converter = TileConverter {};
		let mut reader = mock::TileReader::new("filename.txt").await.unwrap();
		assert!(converter.convert_from(&mut reader).await.is_err())
	}
}
