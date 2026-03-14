#![allow(clippy::cast_sign_loss)]

//! Benchmark for PNG lossless encoding parameters.
//!
//! Usage: cargo run --release --example png_lossless_bench

mod bench_common;

use bench_common::{load_tile_rgb_data, print_header, print_row};
use image::{
	DynamicImage, ImageBuffer, ImageEncoder,
	codecs::png::{CompressionType, FilterType, PngEncoder},
};
use std::time::Instant;

fn encode_png(
	image: &DynamicImage,
	compression: CompressionType,
	filter: FilterType,
) -> Option<(usize, std::time::Duration)> {
	let mut buffer: Vec<u8> = Vec::new();
	let start = Instant::now();
	PngEncoder::new_with_quality(&mut buffer, compression, filter)
		.write_image(image.as_bytes(), image.width(), image.height(), image.color().into())
		.ok()?;
	let elapsed = start.elapsed();
	Some((buffer.len(), elapsed))
}

fn main() {
	let images: Vec<DynamicImage> = load_tile_rgb_data()
		.into_iter()
		.map(|(_label, pixels, w, h)| {
			let buffer = ImageBuffer::from_raw(w as u32, h as u32, pixels).unwrap();
			DynamicImage::ImageRgb8(buffer)
		})
		.collect();

	print_header(&["compression", "filter"]);

	let compressions = [
		(CompressionType::Best, "best"),
		(CompressionType::Default, "default"),
		(CompressionType::Fast, "fast"),
	];
	let filters = [
		(FilterType::NoFilter, "none"),
		(FilterType::Sub, "sub"),
		(FilterType::Up, "up"),
		(FilterType::Avg, "avg"),
		(FilterType::Paeth, "paeth"),
		(FilterType::Adaptive, "adaptive"),
	];

	for &(compression, comp_name) in &compressions {
		for &(filter, filter_name) in &filters {
			let mut sizes = Vec::new();
			let mut total_time = std::time::Duration::ZERO;
			let mut ok = true;

			for image in &images {
				if let Some((size, elapsed)) = encode_png(image, compression, filter) {
					sizes.push(size);
					total_time += elapsed;
				} else {
					ok = false;
					break;
				}
			}

			if !ok {
				continue;
			}

			print_row(&[comp_name, filter_name], &sizes, total_time);
		}
	}
}
