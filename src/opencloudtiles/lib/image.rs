use super::Blob;
use image::{
	codecs::{jpeg, png},
	load_from_memory_with_format, DynamicImage, ImageEncoder, ImageFormat,
};
use webp::Encoder;

pub fn img2png(image: &DynamicImage) -> Blob {
	let mut buffer: Vec<u8> = Vec::new();
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
	
	Blob::from_vec(buffer)
}

pub fn png2img(data: Blob) -> DynamicImage {
	load_from_memory_with_format(data.as_slice(), ImageFormat::Png).unwrap()
}

pub fn img2jpg(image: &DynamicImage) -> Blob {
	let mut buffer: Vec<u8> = Vec::new();
	jpeg::JpegEncoder::new_with_quality(&mut buffer, 95u8)
		.write_image(
			image.as_bytes(),
			image.width(),
			image.height(),
			image.color(),
		)
		.unwrap();
	
		Blob::from_vec(buffer)
}

pub fn jpg2img(data: Blob) -> DynamicImage {
	load_from_memory_with_format(data.as_slice(), ImageFormat::Jpeg).unwrap()
}

pub fn img2webp(image: &DynamicImage) -> Blob {
	let encoder = Encoder::from_image(image).unwrap();
	let memory = encoder.encode(95f32);

	Blob::from_vec(memory.to_vec())
}

pub fn img2webplossless(image: &DynamicImage) -> Blob {
	let encoder = Encoder::from_image(image).unwrap();
	let memory = encoder.encode_lossless();

	Blob::from_vec(memory.to_vec())
}

pub fn webp2img(data: Blob) -> DynamicImage {
	load_from_memory_with_format(data.as_slice(), ImageFormat::WebP).unwrap()
}
