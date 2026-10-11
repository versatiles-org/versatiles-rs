//! Which parts of a tile hold anything.
//!
//! A tile created from a lower-zoom tile is empty exactly where that tile is
//! transparent. Deciding that from the pixels would mean cropping and scaling
//! every candidate first; a [`Mask`] answers it from one bit per pixel, and is
//! small enough to keep for every tile that is ever used as a source.

use versatiles_core::{TileBBox, TileCoord, TileCover};
use versatiles_image::{DynamicImage, DynamicImageTraitInfo};

/// Where a tile has content: a pixel counts as soon as it is not fully
/// transparent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mask {
	/// Every pixel has content. What a tile without an alpha channel is.
	Full,
	/// No pixel has content.
	Empty,
	/// Some pixels have content: one bit per pixel, row by row.
	Bits { width: u32, height: u32, bits: Vec<u64> },
}

impl Mask {
	/// Reads the mask off a decoded tile.
	pub fn from_image(image: &DynamicImage) -> Mask {
		// Anything but 8 bits per channel is not expected after decoding. If it
		// turns up, "full" is the safe answer: a tile is created that might
		// have been left out, never the other way round.
		if !image.color().has_alpha() || image.bits_per_value() != 8 {
			return Mask::Full;
		}

		let (width, height) = (image.width(), image.height());
		let channels = image.channel_count() as usize;
		let alpha = image.as_bytes().iter().skip(channels - 1).step_by(channels);

		let mut bits = vec![0u64; (width as usize * height as usize).div_ceil(64)];
		let mut count = 0usize;
		for (index, alpha) in alpha.enumerate() {
			if *alpha > 0 {
				bits[index / 64] |= 1 << (index % 64);
				count += 1;
			}
		}

		if count == 0 {
			Mask::Empty
		} else if count == width as usize * height as usize {
			Mask::Full
		} else {
			Mask::Bits { width, height, bits }
		}
	}

	/// Bytes this mask takes, for weighing it in a cache.
	pub fn weight(&self) -> u32 {
		match self {
			Mask::Full | Mask::Empty => 0,
			Mask::Bits { bits, .. } => u32::try_from(bits.len() * 8).unwrap_or(u32::MAX),
		}
	}

	/// Whether the tile at `coord` has content anywhere inside `region`.
	///
	/// `region` is a box of tiles at the level of `coord` or above it, lying
	/// inside the area of `coord`. It is widened to whole pixels, and below the
	/// size of one pixel it is that pixel, which is what scaling it up shows.
	pub fn has_content(&self, coord: TileCoord, region: &TileBBox) -> bool {
		let (width, height, bits) = match self {
			Mask::Full => return !region.is_empty(),
			Mask::Empty => return false,
			Mask::Bits { width, height, bits } => (*width, *height, bits),
		};
		let (Ok(x_min), Ok(y_min), Ok(x_max), Ok(y_max)) =
			(region.x_min(), region.y_min(), region.x_max(), region.y_max())
		else {
			return false;
		};

		// How many tiles of the region's level fit along one edge of `coord`.
		let scale = 1u64 << (region.level() - coord.level);
		let pixels = |tile_min: u32, tile_max: u32, origin: u32, size: u32| {
			let size = u64::from(size);
			let offset = u64::from(origin) * scale;
			let from = (u64::from(tile_min) - offset) * size / scale;
			let to = ((u64::from(tile_max) + 1 - offset) * size).div_ceil(scale);
			(from.min(size - 1), to.clamp(from + 1, size))
		};
		let (px_min, px_max) = pixels(x_min, x_max, coord.x, width);
		let (py_min, py_max) = pixels(y_min, y_max, coord.y, height);

		(py_min..py_max).any(|py| {
			(px_min..px_max).any(|px| {
				let index = usize::try_from(py * u64::from(width) + px).expect("a pixel index fits usize");
				bits[index / 64] & (1 << (index % 64)) != 0
			})
		})
	}

	/// Adds the area in which the tile at `coord` has content to `cover`, as
	/// tiles of the level of `cover`.
	///
	/// Exact when one tile of that level is one pixel of this one or smaller;
	/// with coarser tiles, a tile is added as soon as any pixel in it has
	/// content.
	pub fn add_content_to(&self, coord: TileCoord, cover: &mut TileCover) -> anyhow::Result<()> {
		let area = coord.to_tile_bbox().at_level(cover.level());
		match self {
			Mask::Empty => {}
			Mask::Full => cover.insert_bbox(&area)?,
			Mask::Bits { .. } => {
				for tile in area.iter_coords() {
					if self.has_content(coord, &tile.to_tile_bbox()) {
						cover.insert_coord(&tile)?;
					}
				}
			}
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use versatiles_image::DynamicImageTraitConvert;

	use super::*;

	/// 8×8 pixels, content only in the pixel at (5, 2).
	fn one_pixel() -> Mask {
		Mask::from_image(&DynamicImage::from_fn(8, 8, |x, y| {
			[10u8, 20, 30, if (x, y) == (5, 2) { 255 } else { 0 }]
		}))
	}

	fn region(level: u8, x: u32, y: u32) -> TileBBox {
		TileCoord::new(level, x, y).unwrap().to_tile_bbox()
	}

	#[test]
	fn an_image_without_alpha_is_full() {
		let mask = Mask::from_image(&DynamicImage::from_fn(8, 8, |_, _| [1u8, 2, 3]));
		assert_eq!(mask, Mask::Full);
		assert_eq!(mask.weight(), 0);
	}

	#[test]
	fn full_and_empty_alpha_collapse_to_a_flag() {
		assert_eq!(
			Mask::from_image(&DynamicImage::from_fn(8, 8, |_, _| [1u8, 2, 3, 1])),
			Mask::Full
		);
		assert_eq!(
			Mask::from_image(&DynamicImage::from_fn(8, 8, |_, _| [1u8, 2, 3, 0])),
			Mask::Empty
		);
	}

	#[test]
	fn a_faint_pixel_counts_as_content() {
		let mask = Mask::from_image(&DynamicImage::from_fn(8, 8, |x, _| [1u8, u8::from(x == 0)]));
		assert!(matches!(mask, Mask::Bits { .. }));
	}

	/// The tile is 3/4/1. Three levels down, one tile is one pixel.
	#[test]
	fn content_is_found_by_the_tile_that_covers_its_pixel() {
		let mask = one_pixel();
		let coord = TileCoord::new(3, 4, 1).unwrap();

		// One level down: the pixel (5, 2) lies in the upper right quarter.
		assert!(mask.has_content(coord, &region(4, 9, 2)));
		assert!(!mask.has_content(coord, &region(4, 8, 2)));
		assert!(!mask.has_content(coord, &region(4, 9, 3)));

		// Three levels down: exactly the tile of that pixel.
		assert!(mask.has_content(coord, &region(6, 4 * 8 + 5, 8 + 2)));
		assert!(!mask.has_content(coord, &region(6, 4 * 8 + 4, 8 + 2)));

		// Five levels down, a tile is a quarter of a pixel wide: all sixteen
		// tiles inside the pixel have content, their neighbours do not.
		let (x, y) = ((4 * 8 + 5) * 4, (8 + 2) * 4);
		assert!(mask.has_content(coord, &region(8, x, y)));
		assert!(mask.has_content(coord, &region(8, x + 3, y + 3)));
		assert!(!mask.has_content(coord, &region(8, x + 4, y)));
		assert!(!mask.has_content(coord, &region(8, x, y - 1)));

		// The tile itself, and a box of several tiles.
		assert!(mask.has_content(coord, &region(3, 4, 1)));
		assert!(mask.has_content(coord, &TileBBox::from_min_and_max(6, 32, 8, 39, 10).unwrap()));
		assert!(!mask.has_content(coord, &TileBBox::from_min_and_max(6, 32, 11, 39, 15).unwrap()));
	}

	#[test]
	fn content_is_added_to_a_cover_at_pixel_resolution() {
		let coord = TileCoord::new(3, 4, 1).unwrap();

		let mut cover = TileCover::new_empty(6).unwrap();
		one_pixel().add_content_to(coord, &mut cover).unwrap();
		assert_eq!(cover.count_tiles(), 1);
		assert!(cover.includes_coord(&TileCoord::new(6, 37, 10).unwrap()));

		// Coarser than a pixel: the tile that holds the pixel.
		let mut cover = TileCover::new_empty(4).unwrap();
		one_pixel().add_content_to(coord, &mut cover).unwrap();
		assert_eq!(cover.count_tiles(), 1);
		assert!(cover.includes_coord(&TileCoord::new(4, 9, 2).unwrap()));

		let mut cover = TileCover::new_empty(5).unwrap();
		Mask::Full.add_content_to(coord, &mut cover).unwrap();
		assert_eq!(cover.count_tiles(), 16);

		let mut cover = TileCover::new_empty(5).unwrap();
		Mask::Empty.add_content_to(coord, &mut cover).unwrap();
		assert!(cover.is_empty());
	}
}
