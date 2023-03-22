use async_trait::async_trait;

use crate::{
	container::{TileConverterBox, TileConverterTrait, TileReaderBox},
	helper::TileConverterConfig,
};
use std::path::Path;

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
