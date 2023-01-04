mod tile_bbox;
mod tile_bbox_pyramide;
mod tile_converter_config;
mod tile_coords;
mod tile_reader_parameters;

use clap::ValueEnum;
use enumset::EnumSetType;
use hyper::body::Bytes;
use std::ops::Range;
pub use tile_bbox::TileBBox;
pub use tile_bbox_pyramide::TileBBoxPyramide;
pub use tile_converter_config::TileConverterConfig;
pub use tile_coords::{TileCoord2, TileCoord3};
pub use tile_reader_parameters::TileReaderParameters;

#[derive(Clone, Debug, PartialEq, ValueEnum)]
pub enum TileFormat {
	PBF,
	PNG,
	JPG,
	WEBP,
}

#[derive(Debug, EnumSetType, ValueEnum)]
pub enum Precompression {
	Uncompressed,
	Gzip,
	Brotli,
}

#[derive(Clone)]
pub struct Blob(Bytes);
impl Blob {
	pub fn from_vec(vec: Vec<u8>) -> Blob {
		return Blob(Bytes::from(vec));
	}
	pub fn from_slice(slice: &[u8]) -> Blob {
		return Blob(Bytes::copy_from_slice(slice));
	}
	pub fn empty() -> Blob {
		return Blob(Bytes::from(Vec::new()));
	}
	pub fn get_range(&self, range: Range<usize>) -> Blob {
		return Blob(Bytes::from(Vec::from(&self.0[range])));
	}

	pub fn to_bytes(&self) -> Bytes {
		return self.0.clone();
	}
	pub fn as_slice(&self) -> &[u8] {
		return self.0.as_ref();
	}
	pub fn to_vec(&self) -> Vec<u8> {
		return self.0.to_vec();
	}

	pub fn len(&self) -> usize {
		return self.0.len();
	}
}
