use super::types::{BlockIndex, CloudTilesSrc, FileHeader};
use crate::opencloudtiles::{
	containers::abstract_container,
	types::{TileCoord3, TileData, TileReaderParameters},
};
use std::path::PathBuf;

pub struct TileReader {
	reader: CloudTilesSrc,
	parameters: TileReaderParameters,
}

impl TileReader {
	pub fn new(mut reader: CloudTilesSrc) -> TileReader {
		let header = FileHeader::read(&mut reader);
		println!("{:?}", header);
		let block_index = BlockIndex::from_brotli_vec(&reader.read_range(&header.blocks_range));
		let bbox_pyramide = block_index.get_bbox_pyramide();
		let parameters = TileReaderParameters::new(header.tile_format, bbox_pyramide);
		return TileReader { reader, parameters };
	}
}

impl abstract_container::TileReader for TileReader {
	fn from_file(filename: &PathBuf) -> Box<dyn abstract_container::TileReader>
	where
		Self: Sized,
	{
		let reader = CloudTilesSrc::from_file(filename);
		let tile_reader = TileReader::new(reader);
		return Box::new(tile_reader);
	}
	fn get_meta(&self) -> &[u8] {
		panic!();
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		panic!();
	}
	fn get_tile_data(&self, coord: &TileCoord3) -> Option<TileData> {
		panic!();
	}
}
