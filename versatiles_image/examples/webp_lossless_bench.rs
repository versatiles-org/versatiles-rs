#![allow(clippy::cast_sign_loss)]

//! Benchmark for WebP lossless encoding parameters.
//!
//! Usage: cargo run --release --example webp_lossless_bench

mod bench_common;

use bench_common::{load_tile_rgb_data, print_header, print_row};
use libwebp_sys::{
	WebPConfig, WebPEncode, WebPFree, WebPImageHint, WebPMemoryWrite, WebPMemoryWriter, WebPMemoryWriterClear,
	WebPMemoryWriterInit, WebPPicture, WebPPictureFree, WebPPictureImportRGB,
};
use std::time::Instant;

fn encode_lossless(
	data: &[u8],
	width: i32,
	height: i32,
	method: i32,
	quality: f32,
) -> Option<(usize, std::time::Duration)> {
	unsafe {
		let mut config = WebPConfig::new().ok()?;
		config.lossless = 1;
		config.method = method;
		config.quality = quality;
		config.exact = 0;
		config.image_hint = WebPImageHint::WEBP_HINT_GRAPH;

		let mut picture = WebPPicture::new().ok()?;
		picture.use_argb = 1;
		picture.width = width;
		picture.height = height;

		let stride = width * 3;
		if WebPPictureImportRGB(&raw mut picture, data.as_ptr(), stride) == 0 {
			WebPPictureFree(&raw mut picture);
			return None;
		}

		let mut writer: WebPMemoryWriter = std::mem::zeroed();
		WebPMemoryWriterInit(&raw mut writer);
		picture.writer = Some(WebPMemoryWrite);
		picture.custom_ptr = (&raw mut writer).cast();

		let start = Instant::now();
		let ok = WebPEncode(&raw const config, &raw mut picture);
		let elapsed = start.elapsed();

		WebPPictureFree(&raw mut picture);

		if ok == 0 {
			WebPMemoryWriterClear(&raw mut writer);
			return None;
		}

		let size = writer.size;
		WebPFree(writer.mem.cast());
		Some((size, elapsed))
	}
}

fn main() {
	let images = load_tile_rgb_data();

	print_header(&["method", "quality"]);

	let methods = [0, 1, 2, 3, 4, 5, 6];
	let qualities = [0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0];

	for &method in &methods {
		for &quality in &qualities {
			let mut sizes = Vec::new();
			let mut total_time = std::time::Duration::ZERO;
			let mut ok = true;

			for (_label, pixels, w, h) in &images {
				if let Some((size, elapsed)) = encode_lossless(pixels, *w, *h, method, quality) {
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

			let method_str = method.to_string();
			let quality_str = quality.to_string();
			print_row(&[&method_str, &quality_str], &sizes, total_time);
		}
	}
}
