use crate::{TileConverterBox, TileConverterTrait, TileReaderBox};
use async_trait::async_trait;
use std::path::Path;
use versatiles_shared::TileConverterConfig;

pub struct TileConverter;

#[async_trait]
impl TileConverterTrait for TileConverter {
	fn new(_filename: &Path, _config: TileConverterConfig) -> TileConverterBox
	where
		Self: Sized,
	{
		Box::new(Self {})
	}
	async fn convert_from(&mut self, _reader: &mut TileReaderBox) {}
}

#[cfg(test)]
mod tests {
	use crate::{dummy, TileConverterTrait, TileReaderTrait};
	use futures::executor::block_on;
	use std::path::Path;
	use versatiles_shared::TileConverterConfig;

	#[test]
	fn test1() {
		let _converter = dummy::TileConverter::new(Path::new("filename.txt"), TileConverterConfig::empty());
	}

	#[test]
	fn test2() {
		let mut converter = dummy::TileConverter {};
		let mut reader = block_on(dummy::TileReader::new("filename.txt")).unwrap();
		block_on(converter.convert_from(&mut reader));
	}
}
