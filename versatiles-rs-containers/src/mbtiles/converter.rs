use crate::{
	containers::{TileConverterBox, TileConverterTrait, TileReaderBox},
	shared::{Error, Result, TileConverterConfig},
};
use async_trait::async_trait;

pub struct TileConverter;

#[async_trait]
impl TileConverterTrait for TileConverter {
	async fn new(_filename: &str, _config: TileConverterConfig) -> Result<TileConverterBox>
	where
		Self: Sized,
	{
		Err(Error::new("conversion to mbtiles is not supported"))
	}
	async fn convert_from(&mut self, _reader: &mut TileReaderBox) -> Result<()> {
		Err(Error::new("conversion to mbtiles is not supported"))
	}
}

#[cfg(test)]
mod tests {
	use super::TileConverter;
	use crate::{
		containers::{dummy, TileConverterTrait, TileReaderTrait},
		shared::TileConverterConfig,
	};
	use futures::executor::block_on;

	#[test]
	fn panic1() {
		assert!(block_on(TileConverter::new("filename.txt", TileConverterConfig::new_full())).is_err());
	}

	#[test]
	#[should_panic]
	fn panic2() {
		let mut converter = TileConverter {};
		let mut reader = block_on(dummy::TileReader::new("filename.txt")).unwrap();
		block_on(converter.convert_from(&mut reader)).unwrap();
	}
}
