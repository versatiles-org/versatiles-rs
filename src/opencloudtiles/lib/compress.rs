use super::Blob;
use brotli::{enc::BrotliEncoderParams, BrotliCompress, BrotliDecompress};
use flate2::{
	bufread::{GzDecoder, GzEncoder},
	Compression,
};
use std::io::{Cursor, Read};

use clap::ValueEnum;
use enumset::EnumSetType;

#[derive(Debug, EnumSetType, ValueEnum)]
pub enum Precompression {
	Uncompressed,
	Gzip,
	Brotli,
}

#[allow(dead_code)]
pub fn compress(data: Blob, precompression: &Precompression) -> Blob {
	match precompression {
		Precompression::Uncompressed => data,
		Precompression::Gzip => compress_gzip(data),
		Precompression::Brotli => compress_brotli(data),
	}
}

pub fn decompress(data: Blob, precompression: &Precompression) -> Blob {
	match precompression {
		Precompression::Uncompressed => data,
		Precompression::Gzip => decompress_gzip(data),
		Precompression::Brotli => decompress_brotli(data),
	}
}

pub fn compress_gzip(data: Blob) -> Blob {
	let mut result: Vec<u8> = Vec::new();
	GzEncoder::new(data.as_slice(), Compression::best())
		.read_to_end(&mut result)
		.expect("Error in compress_gzip");
	return Blob::from_vec(result);
}

pub fn decompress_gzip(data: Blob) -> Blob {
	let mut result: Vec<u8> = Vec::new();
	GzDecoder::new(data.as_slice())
		.read_to_end(&mut result)
		.expect("Error in decompress_gzip");
	return Blob::from_vec(result);
}

pub fn compress_brotli(data: Blob) -> Blob {
	let mut params = BrotliEncoderParams::default();
	params.quality = 11;
	params.size_hint = data.len();
	let mut cursor = Cursor::new(data.as_slice());
	let mut result: Vec<u8> = Vec::new();
	BrotliCompress(&mut cursor, &mut result, &params).expect("Error in compress_brotli");
	return Blob::from_vec(result);
}

pub fn decompress_brotli(data: Blob) -> Blob {
	let mut cursor = Cursor::new(data.as_slice());
	let mut result: Vec<u8> = Vec::new();
	BrotliDecompress(&mut cursor, &mut result).expect("Error in decompress_brotli");
	return Blob::from_vec(result);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn verify_brotli() {
		let data1 = random_data(100000);
		let data2 = decompress_brotli(compress_brotli(data1.clone()));
		assert_eq!(data1, data2);
	}

	#[test]
	fn verify_gzip() {
		let data1 = random_data(100000);
		let data2 = decompress_gzip(compress_gzip(data1.clone()));
		assert_eq!(data1, data2);
	}

	fn random_data(size: usize) -> Blob {
		let mut vec: Vec<u8> = Vec::new();
		vec.resize(size, 0);
		for i in 0..size {
			vec[i] = (((i as f64 + 1.78123).cos() * 6513814013423.4538471).fract() * 256f64) as u8;
		}
		return Blob::from_vec(vec);
	}
}
