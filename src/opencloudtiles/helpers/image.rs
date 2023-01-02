use crate::opencloudtiles::types::TileData;
use image::{
	codecs::{
		jpeg::JpegEncoder,
		png::{self, PngEncoder},
	},
	load_from_memory_with_format, DynamicImage, ImageEncoder, ImageFormat,
};
use webp::Encoder;

pub fn compress_png(image: &DynamicImage) -> TileData {
	let mut buffer: TileData = Vec::new();
	PngEncoder::new_with_quality(&mut buffer, png::CompressionType::Best, png::FilterType::Adaptive)
		.write_image(image.as_bytes(), image.width(), image.height(), image.color())
		.unwrap();
	return buffer;
}

pub fn decompress_png(data: &TileData) -> DynamicImage {
	load_from_memory_with_format(data, ImageFormat::Png).unwrap()
}

pub fn compress_jpg(image: &DynamicImage) -> TileData {
	let mut buffer: TileData = Vec::new();
	JpegEncoder::new_with_quality(&mut buffer, 95u8)
		.write_image(image.as_bytes(), image.width(), image.height(), image.color())
		.unwrap();
	return buffer;
}

pub fn decompress_jpg(data: &TileData) -> DynamicImage {
	load_from_memory_with_format(data, ImageFormat::Jpeg).unwrap()
}

pub fn compress_webp(image: &DynamicImage) -> TileData {
	let encoder = Encoder::from_image(&image).unwrap();
	let memory = encoder.encode(95f32);
	return memory.to_vec();
}

pub fn compress_webp_lossless(image: &DynamicImage) -> TileData {
	let encoder = Encoder::from_image(&image).unwrap();
	let memory = encoder.encode_lossless();
	return memory.to_vec();
}

pub fn decompress_webp(data: &TileData) -> DynamicImage {
	load_from_memory_with_format(data, ImageFormat::WebP).unwrap()
}
