use std::io::{Cursor, Read};

use brotli::{enc::BrotliEncoderParams, BrotliCompress, BrotliDecompress};
use flate2::{
	bufread::{GzDecoder, GzEncoder},
	Compression,
};

use super::Tile;

pub fn compress_gzip(data: &Tile) -> Tile {
	let mut result: Tile = Vec::new();
	GzEncoder::new(data.as_slice(), Compression::best())
		.read_to_end(&mut result)
		.unwrap();
	return result;
}

pub fn decompress_gzip(data: &Tile) -> Tile {
	let mut result: Tile = Vec::new();
	GzDecoder::new(data.as_slice())
		.read_to_end(&mut result)
		.unwrap();
	return result;
}

pub fn compress_brotli(data: &Tile) -> Tile {
	let mut params = BrotliEncoderParams::default();
	params.quality = 11;
	params.size_hint = data.len();
	let mut cursor = Cursor::new(data);
	let mut result: Tile = Vec::new();
	BrotliCompress(&mut cursor, &mut result, &params).unwrap();
	return result;
}

pub fn decompress_brotli(data: &Tile) -> Tile {
	let mut cursor = Cursor::new(data);
	let mut result: Tile = Vec::new();
	BrotliDecompress(&mut cursor, &mut result).unwrap();
	return result;
}
