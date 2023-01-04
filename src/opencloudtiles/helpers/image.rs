use crate::opencloudtiles::types::TileData;
use image::{
	codecs::{jpeg, png},
	load_from_memory_with_format, DynamicImage, ImageEncoder, ImageFormat,
};
use webp::Encoder;

pub fn img2png(image: &DynamicImage) -> TileData {
	let mut buffer: TileData = Vec::new();
	png::PngEncoder::new_with_quality(
		&mut buffer,
		png::CompressionType::Best,
		png::FilterType::Adaptive,
	)
	.write_image(
		image.as_bytes(),
		image.width(),
		image.height(),
		image.color(),
	)
	.unwrap();
	return buffer;
}

pub fn png2img(data: &TileData) -> DynamicImage {
	load_from_memory_with_format(data, ImageFormat::Png).unwrap()
}

pub fn img2jpg(image: &DynamicImage) -> TileData {
	let mut buffer: TileData = Vec::new();
	jpeg::JpegEncoder::new_with_quality(&mut buffer, 95u8)
		.write_image(
			image.as_bytes(),
			image.width(),
			image.height(),
			image.color(),
		)
		.unwrap();
	return buffer;
}

pub fn jpg2img(data: &TileData) -> DynamicImage {
	load_from_memory_with_format(data, ImageFormat::Jpeg).unwrap()
}

pub fn img2webp(image: &DynamicImage) -> TileData {
	let encoder = Encoder::from_image(&image).unwrap();
	let memory = encoder.encode(95f32);
	return memory.to_vec();
}

pub fn img2webplossless(image: &DynamicImage) -> TileData {
	let encoder = Encoder::from_image(&image).unwrap();
	let memory = encoder.encode_lossless();
	return memory.to_vec();
}

pub fn webp2img(data: &TileData) -> DynamicImage {
	load_from_memory_with_format(data, ImageFormat::WebP).unwrap()
}
