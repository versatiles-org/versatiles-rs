//! Cutting the region of a lower-zoom tile that a higher-zoom tile covers, and
//! scaling it up to a full tile.

use anyhow::{Result, ensure};
use versatiles_core::TileCoord;
use versatiles_derive::context;
use versatiles_image::{DynamicImage, traits::DynamicImageTraitOperation};

/// How the region of a lower-zoom tile becomes a full tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scaling {
	/// Every channel is resampled by itself, which is right for colors.
	Color,
}

impl Scaling {
	/// Cuts the area of `coord_dst` out of the tile at `coord_src` and scales it
	/// up to the size of a tile.
	pub fn extract(self, image_src: &DynamicImage, coord_src: TileCoord, coord_dst: TileCoord) -> Result<DynamicImage> {
		match self {
			Scaling::Color => extract_image(image_src, coord_src, coord_dst),
		}
	}
}

/// Extracts and upscales a sub-region from a source tile to create a higher-zoom tile.
///
/// Given a source tile at a lower zoom level, this function calculates which portion
/// of that tile corresponds to the requested destination tile, extracts it, and
/// scales it up to full tile dimensions.
///
/// ## Parameters
/// - `image_src`: The source tile image to extract from
/// - `coord_src`: The coordinate of the source tile (lower zoom level)
/// - `coord_dst`: The coordinate of the destination tile (higher zoom level)
///
/// ## Extraction Logic
///
/// Each increase in zoom level divides a tile into 4 quadrants. For a level difference
/// of N, the source tile is divided into 2^N × 2^N sub-tiles.
///
/// Example: Extracting z16/1000/2000 from z14/250/500 (level_diff = 2):
/// - Scale factor: 2^2 = 4, so source is divided into 4×4 = 16 sub-regions
/// - Sub-region coordinates: (1000 % 4, 2000 % 4) = (0, 0) → top-left quarter
/// - Extract 64×64 pixels (256/4) and upscale to 256×256
#[context("extracting image for tile {:?}", coord_dst)]
fn extract_image(image_src: &DynamicImage, coord_src: TileCoord, coord_dst: TileCoord) -> Result<DynamicImage> {
	let level_diff = i32::from(coord_dst.level) - i32::from(coord_src.level);

	ensure!(level_diff >= 0, "difference in levels must be non-negative");

	// Same level: return a clone (no extraction needed)
	if level_diff == 0 {
		return Ok((*image_src).clone());
	}

	// Calculate extraction parameters
	// scale = 2^level_diff (e.g., level_diff=2 → 4×4 grid of sub-tiles)
	let scale = 1 << level_diff;
	let tile_size = image_src.width(); // Assume square tiles
	let sub_size = f64::from(tile_size) / f64::from(scale);

	// Find which sub-tile within the grid corresponds to coord_dst
	let tile_offset_x = f64::from(coord_dst.x % scale);
	let tile_offset_y = f64::from(coord_dst.y % scale);
	let x0 = tile_offset_x * sub_size;
	let y0 = tile_offset_y * sub_size;

	// Extract the sub-region and upscale to full tile size
	image_src.extract(x0, y0, sub_size, sub_size, tile_size, tile_size)
}

#[cfg(test)]
#[expect(clippy::cast_possible_truncation, reason = "test data is built from literal values")]
mod tests {
	use versatiles_image::DynamicImageTraitConvert;

	use super::*;

	fn gradient_image() -> DynamicImage {
		DynamicImage::from_fn(256, 256, |x, y| [x as u8, y as u8, 255 - x as u8])
	}

	#[test]
	fn test_extract_image_same_level() -> Result<()> {
		let image = gradient_image();
		let coord = TileCoord::new(2, 1, 1)?;
		let result = Scaling::Color.extract(&image, coord, coord)?;
		assert_eq!(result.width(), image.width());
		assert_eq!(result.height(), image.height());
		Ok(())
	}

	#[test]
	fn test_extract_image_higher_level() -> Result<()> {
		let image = gradient_image();
		let coord_src = TileCoord::new(2, 1, 1)?;
		let coord_dst = TileCoord::new(3, 2, 2)?;
		let result = Scaling::Color.extract(&image, coord_src, coord_dst)?;
		// Result should be same size as source (upscaled extract)
		assert_eq!(result.width(), image.width());
		assert_eq!(result.height(), image.height());
		Ok(())
	}
}
