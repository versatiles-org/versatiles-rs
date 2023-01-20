use crate::{
	container::{TileConverterBox, TileConverterTrait, TileReaderBox},
	helper::TileConverterConfig,
};
use std::path::Path;

pub struct TileConverter;
impl TileConverterTrait for TileConverter {
	fn new(_filename: &Path, _config: TileConverterConfig) -> TileConverterBox
	where
		Self: Sized,
	{
		panic!()
	}
	fn convert_from(&mut self, _reader: &mut TileReaderBox) {
		panic!()
	}
}
