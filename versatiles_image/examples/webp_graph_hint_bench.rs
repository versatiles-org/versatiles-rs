#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]

//! Focused benchmark: does `WEBP_HINT_GRAPH` help lossless DEM encoding at method=6?
//!
//! Compares, per real elevation tile, lossless WebP at method=6/quality=100 under
//! two image hints: DEFAULT (≈ current production `encode_lossless`, which sets no
//! hint) vs GRAPH (proposed for DEM). Reports per-tile sizes, totals, % delta and time.
//!
//! Usage: cargo run --release --example webp_graph_hint_bench

mod bench_common;

use bench_common::load_tile_rgb_data;
use libwebp_sys::{
	WebPConfig, WebPEncode, WebPFree, WebPImageHint, WebPMemoryWrite, WebPMemoryWriter, WebPMemoryWriterClear,
	WebPMemoryWriterInit, WebPPicture, WebPPictureFree, WebPPictureImportRGB,
};
use std::time::{Duration, Instant};

fn encode_lossless_hint(
	data: &[u8],
	width: i32,
	height: i32,
	method: i32,
	hint: WebPImageHint,
) -> Option<(usize, Duration)> {
	unsafe {
		let mut config = WebPConfig::new().ok()?;
		config.lossless = 1;
		config.method = method;
		config.quality = 100.0;
		config.exact = 1; // match production (irrelevant for opaque RGB, kept for parity)
		config.image_hint = hint;

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

	// Does the GRAPH hint ever help? Test across methods to confirm the hint is
	// actually wired (lower methods should react) and quantify the method=6 verdict.
	println!("method\tdefault\tgraph\tdelta\tdelta%\ttime_def_ms\ttime_graph_ms");
	for &method in &[0i32, 1, 2, 3, 4, 5, 6] {
		let (mut tot_def, mut tot_graph) = (0usize, 0usize);
		let (mut time_def, mut time_graph) = (Duration::ZERO, Duration::ZERO);
		for (_label, pixels, w, h) in &images {
			let (sd, td) =
				encode_lossless_hint(pixels, *w, *h, method, WebPImageHint::WEBP_HINT_DEFAULT).expect("default encode");
			let (sg, tg) =
				encode_lossless_hint(pixels, *w, *h, method, WebPImageHint::WEBP_HINT_GRAPH).expect("graph encode");
			tot_def += sd;
			tot_graph += sg;
			time_def += td;
			time_graph += tg;
		}
		let delta = tot_graph as i64 - tot_def as i64;
		let pct = (delta as f64 / tot_def as f64) * 100.0;
		println!(
			"{method}\t{tot_def}\t{tot_graph}\t{delta:+}\t{pct:+.2}%\t{:.1}\t{:.1}",
			time_def.as_secs_f64() * 1000.0,
			time_graph.as_secs_f64() * 1000.0
		);
	}
	println!("\n(totals over all tiles; negative delta% = GRAPH smaller; quality=100, exact=1)");
}
