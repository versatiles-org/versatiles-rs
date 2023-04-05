use super::Blob;
use brotli::{enc::BrotliEncoderParams, BrotliCompress, BrotliDecompress};
use clap::ValueEnum;
use enumset::EnumSetType;
use flate2::{
	bufread::{GzDecoder, GzEncoder},
	Compression,
};
use std::io::{Cursor, Read};

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

	Blob::from(result)
}

pub fn decompress_gzip(data: Blob) -> Blob {
	let mut result: Vec<u8> = Vec::new();
	GzDecoder::new(data.as_slice())
		.read_to_end(&mut result)
		.expect("Error in decompress_gzip");

	Blob::from(result)
}

pub fn compress_brotli(data: Blob) -> Blob {
	let params = BrotliEncoderParams {
		quality: 11,
		size_hint: data.len(),
		..Default::default()
	};
	let mut cursor = Cursor::new(data.as_slice());
	let mut result: Vec<u8> = Vec::new();
	BrotliCompress(&mut cursor, &mut result, &params).expect("Error in compress_brotli");

	Blob::from(result)
}

pub fn decompress_brotli(data: Blob) -> Blob {
	let mut cursor = Cursor::new(data.as_slice());
	let mut result: Vec<u8> = Vec::new();
	BrotliDecompress(&mut cursor, &mut result).expect("Error in decompress_brotli");

	Blob::from(result)
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
		(0..size).for_each(|i| {
			vec[i] = (((i as f64 + 1.78123).cos() * 6_513_814_013_423.454).fract() * 256f64) as u8;
		});

		Blob::from(vec)
	}
}
