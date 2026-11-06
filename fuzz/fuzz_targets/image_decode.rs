//! Decode arbitrary bytes as a raster tile.
//!
//! The first byte picks the format, the rest is the image. WebP goes through
//! libwebp, which is C: a bug there is a memory-safety bug rather than a panic,
//! and nothing in Rust's checks would catch it. libwebp has had one before
//! (CVE-2023-4863, a heap overflow in the lossless decoder). PNG and JPEG go
//! through the `image` crate, where the risk is an allocation the header
//! claimed but the data never backs.

#![no_main]

use libfuzzer_sys::fuzz_target;
use versatiles_core::{Blob, TileFormat};
use versatiles_image::format::decode;

fuzz_target!(|data: &[u8]| {
	let Some((&selector, image)) = data.split_first() else {
		return;
	};
	let format = match selector % 3 {
		0 => TileFormat::WEBP,
		1 => TileFormat::PNG,
		_ => TileFormat::JPG,
	};
	let _ = decode(&Blob::from(image.to_vec()), format);
});
