//! Shared test utilities and fixtures for the versatiles_container crate.
//!
//! This module provides common test helpers that can be reused across test modules,
//! including downstream crates that pull `versatiles_container` with the `test`
//! feature enabled.

use crate::TileSource;
use anyhow::Result;
use versatiles_core::TileBBox;
use versatiles_image::{DynamicImage, ImageBuffer};

/// Creates a tiny 2x2 RGB test image with known pixel values.
///
/// Pixel layout:
/// - (0,0): red (255, 0, 0)
/// - (1,0): green (0, 255, 0)
/// - (0,1): blue (0, 0, 255)
/// - (1,1): misc (10, 20, 30)
#[must_use]
pub fn tiny_rgb_image() -> DynamicImage {
	let data = vec![
		255, 0, 0, // red
		0, 255, 0, // green
		0, 0, 255, // blue
		10, 20, 30, // misc
	];
	DynamicImage::ImageRgb8(ImageBuffer::from_vec(2, 2, data).expect("vec->img"))
}

/// Asserts that a source's `tile_coord_stream` and `tile_stream` produce the
/// same number of items for the given `bbox`.
///
/// This invariant is required by the [`TileSource`](crate::TileSource) trait:
/// any implementation that diverges between the two streams will silently
/// break the converter's progress accounting (and any other code that counts
/// via the lighter `tile_coord_stream`). See the trait docs for the rationale.
///
/// # Panics
/// Panics with a useful message when the two counts disagree.
pub async fn assert_stream_counts_agree(source: &dyn TileSource, bbox: TileBBox) -> Result<()> {
	let coord_count = source.tile_coord_stream(bbox).await?.drain_and_count().await;
	let tile_count = source.tile_stream(bbox).await?.to_vec().await.len() as u64;
	assert_eq!(
		coord_count, tile_count,
		"TileSource invariant violated for bbox {bbox:?}: tile_coord_stream yielded {coord_count}, tile_stream yielded {tile_count}",
	);
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_image::GenericImageView;

	#[test]
	fn tiny_rgb_image_has_expected_dimensions() {
		let img = tiny_rgb_image();
		assert_eq!(img.dimensions(), (2, 2));
	}

	#[test]
	fn tiny_rgb_image_has_expected_pixels() {
		let img = tiny_rgb_image();
		let p00 = img.get_pixel(0, 0);
		let p10 = img.get_pixel(1, 0);
		let p01 = img.get_pixel(0, 1);
		let p11 = img.get_pixel(1, 1);

		assert_eq!(p00.0[0..3], [255, 0, 0]); // red
		assert_eq!(p10.0[0..3], [0, 255, 0]); // green
		assert_eq!(p01.0[0..3], [0, 0, 255]); // blue
		assert_eq!(p11.0[0..3], [10, 20, 30]); // misc
	}
}
