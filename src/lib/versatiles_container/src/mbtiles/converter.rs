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
		panic!()
	}
	async fn convert_from(&mut self, _reader: &mut TileReaderBox) {
		panic!()
	}
}
