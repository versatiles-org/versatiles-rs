use super::types::TileData;
use brotli::{enc::BrotliEncoderParams, BrotliCompress, BrotliDecompress};
use flate2::{
	bufread::{GzDecoder, GzEncoder},
	Compression,
};
use std::io::{Cursor, Read};

pub fn compress_gzip(data: &TileData) -> TileData {
	let mut result: TileData = Vec::new();
	GzEncoder::new(data.as_slice(), Compression::best())
		.read_to_end(&mut result)
		.expect("Error in compress_gzip");
	return result;
}

pub fn decompress_gzip(data: &TileData) -> TileData {
	let mut result: TileData = Vec::new();
	GzDecoder::new(data.as_slice())
		.read_to_end(&mut result)
		.expect("Error in decompress_gzip");
	return result;
}

pub fn compress_brotli(data: &TileData) -> TileData {
	println!("compress_brotli: ({}) {:?}", data.len(), data);
	let mut params = BrotliEncoderParams::default();
	params.quality = 11;
	params.size_hint = data.len();
	let mut cursor = Cursor::new(data);
	let mut result: TileData = Vec::new();
	BrotliCompress(&mut cursor, &mut result, &params).expect("Error in compress_brotli");
	println!("result: ({}) {:?}", result.len(), result);
	return result;
}

pub fn decompress_brotli(data: &TileData) -> TileData {
	println!("decompress_brotli: ({}) {:?}", data.len(), data);
	let mut cursor = Cursor::new(data);
	let mut result: TileData = Vec::new();
	BrotliDecompress(&mut cursor, &mut result).expect("Error in decompress_brotli");
	println!("result: ({}) {:?}", result.len(), result);
	return result;
}
