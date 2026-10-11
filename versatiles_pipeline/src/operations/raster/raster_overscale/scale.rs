//! Cutting the region of a lower-zoom tile that a higher-zoom tile covers, and
//! scaling it up to a full tile.

use anyhow::{Result, bail, ensure};
use versatiles_container::Tile;
use versatiles_core::{TileCoord, TileFormat, TileJSON, TileSchema};
use versatiles_derive::context;
use versatiles_image::{DynamicImage, ImageBuffer, Rgb, Rgba, traits::DynamicImageTraitOperation};

/// How the region of a lower-zoom tile becomes a full tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scaling {
	/// Every channel is resampled by itself, which is right for colors.
	Color,
	/// The three color channels are one number, a height, and that number is
	/// what gets resampled.
	///
	/// An elevation tile stores a height as `R·65536 + G·256 + B`. Resampled
	/// channel by channel, the middle between `(0, 0, 255)` and `(0, 1, 0)` —
	/// two heights one step apart — comes out as `(0, 0, 128)` or thereabouts:
	/// half the height, a pit along every contour where a byte carries over.
	Elevation,
}

impl Scaling {
	/// The scaling that suits the tiles of a source, going by the tile schema
	/// in its metadata.
	///
	/// Both elevation schemas store the height as the same 24-bit number and
	/// differ only in what one step of it is worth, so they scale alike.
	pub fn for_source(tilejson: &TileJSON) -> Result<Scaling> {
		match tilejson.tile_schema {
			Some(TileSchema::RasterDEMMapbox | TileSchema::RasterDEMTerrarium) => Ok(Scaling::Elevation),
			Some(TileSchema::RasterDEMVersatiles) => bail!(
				"cannot upscale tiles of the schema 'dem/versatiles': only 'dem/mapbox' and 'dem/terrarium' elevation tiles are supported"
			),
			_ => Ok(Scaling::Color),
		}
	}

	/// Cuts the area of `coord_dst` out of the tile at `coord_src` and scales it
	/// up to the size of a tile.
	pub fn extract(self, image_src: &DynamicImage, coord_src: TileCoord, coord_dst: TileCoord) -> Result<DynamicImage> {
		match self {
			Scaling::Color => extract_image(image_src, coord_src, coord_dst),
			Scaling::Elevation => extract_elevation(image_src, coord_src, coord_dst),
		}
	}

	/// Creates the tile at `coord_dst` from the tile at `coord_src`.
	///
	/// An elevation tile is marked to be encoded without loss: a lossy encoder
	/// changes the low bits of a channel, and here those are heights.
	pub fn tile(
		self,
		image_src: &DynamicImage,
		coord_src: TileCoord,
		coord_dst: TileCoord,
		format: TileFormat,
	) -> Result<Tile> {
		let mut tile = Tile::from_image(self.extract(image_src, coord_src, coord_dst)?, format)?;
		if self == Scaling::Elevation {
			tile.set_format_quality(Some(100));
		}
		Ok(tile)
	}
}

/// The region of the tile at `coord_src` that the tile at `coord_dst` covers:
/// its left and top edge and its size, in pixels of an image `size` wide.
fn region(size: u32, coord_src: TileCoord, coord_dst: TileCoord) -> Result<(f64, f64, f64)> {
	ensure!(
		coord_dst.level >= coord_src.level,
		"difference in levels must be non-negative"
	);
	let scale = 1u32 << (coord_dst.level - coord_src.level);
	let sub_size = f64::from(size) / f64::from(scale);
	Ok((
		f64::from(coord_dst.x % scale) * sub_size,
		f64::from(coord_dst.y % scale) * sub_size,
		sub_size,
	))
}

/// Like [`extract_image`], for elevation tiles: the height of every pixel is
/// interpolated between the four pixels around it.
///
/// Bilinear on purpose. A sharper filter overshoots at a steep edge, which for
/// a photo is a halo and for terrain is a ridge and a ditch that are not
/// there. Between four heights, the result is never above the highest or
/// below the lowest of them.
///
/// A transparent pixel is one without data, and takes no part: next to it the
/// height comes from the pixels that have data, and where none of the four
/// has any, the result has none either.
#[context("extracting elevation for tile {:?}", coord_dst)]
fn extract_elevation(image_src: &DynamicImage, coord_src: TileCoord, coord_dst: TileCoord) -> Result<DynamicImage> {
	let (width, height) = (image_src.width(), image_src.height());
	ensure!(width > 0 && height > 0, "cannot upscale an empty image");

	// Heights as one number per pixel; `None` where there is no data.
	let heights: Vec<Option<f64>> = match image_src {
		DynamicImage::ImageRgb8(image) => image.pixels().map(|p| Some(decode(p.0[0], p.0[1], p.0[2]))).collect(),
		DynamicImage::ImageRgba8(image) => image
			.pixels()
			.map(|p| (p.0[3] > 0).then(|| decode(p.0[0], p.0[1], p.0[2])))
			.collect(),
		_ => bail!("elevation tiles must be RGB or RGBA with 8 bits per channel"),
	};
	let has_alpha = matches!(image_src, DynamicImage::ImageRgba8(_));

	let (x0, y0, sub_size) = region(width, coord_src, coord_dst)?;
	let step = sub_size / f64::from(width);
	// Where the center of a pixel of the result lies in the source, counted
	// from the center of the source's first pixel.
	let position = |index: u32, origin: f64, limit: u32| {
		let at = (origin + (f64::from(index) + 0.5) * step - 0.5).clamp(0.0, f64::from(limit - 1));
		#[expect(
			clippy::cast_possible_truncation,
			clippy::cast_sign_loss,
			reason = "`at` is clamped to 0..=limit-1"
		)]
		let low = at.floor() as u32;
		(low, (low + 1).min(limit - 1), at - at.floor())
	};
	let sample = |x: u32, y: u32| heights[(y * width + x) as usize];

	let interpolate = |x: u32, y: u32| -> Option<f64> {
		let (x_low, x_high, x_part) = position(x, x0, width);
		let (y_low, y_high, y_part) = position(y, y0, height);
		let mut sum = 0.0;
		let mut weight_sum = 0.0;
		for (sx, sy, weight) in [
			(x_low, y_low, (1.0 - x_part) * (1.0 - y_part)),
			(x_high, y_low, x_part * (1.0 - y_part)),
			(x_low, y_high, (1.0 - x_part) * y_part),
			(x_high, y_high, x_part * y_part),
		] {
			if let Some(value) = sample(sx, sy) {
				sum += value * weight;
				weight_sum += weight;
			}
		}
		// The nearest of the four decides whether there is data here at all.
		// Without that, data would grow by half a pixel into every gap.
		let nearest = sample(
			if x_part < 0.5 { x_low } else { x_high },
			if y_part < 0.5 { y_low } else { y_high },
		);
		(nearest.is_some() && weight_sum > 0.0).then(|| sum / weight_sum)
	};

	Ok(if has_alpha {
		DynamicImage::ImageRgba8(ImageBuffer::from_fn(width, height, |x, y| match interpolate(x, y) {
			Some(value) => {
				let [r, g, b] = encode(value);
				Rgba([r, g, b, 255])
			}
			None => Rgba([0, 0, 0, 0]),
		}))
	} else {
		DynamicImage::ImageRgb8(ImageBuffer::from_fn(width, height, |x, y| {
			Rgb(encode(interpolate(x, y).unwrap_or(0.0)))
		}))
	})
}

/// The 24-bit number an elevation tile stores in a pixel.
fn decode(r: u8, g: u8, b: u8) -> f64 {
	f64::from(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
}

/// The pixel that stores `value`, rounded to the nearest step.
fn encode(value: f64) -> [u8; 3] {
	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		reason = "the value is clamped to 24 bits"
	)]
	let raw = value.round().clamp(0.0, f64::from(0x00FF_FFFF_u32)) as u32;
	let [_, r, g, b] = raw.to_be_bytes();
	[r, g, b]
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

	// --- elevation ---

	fn raw(pixel: &[u8]) -> u32 {
		u32::from(pixel[0]) << 16 | u32::from(pixel[1]) << 8 | u32::from(pixel[2])
	}

	fn heights(image: &DynamicImage) -> Vec<u32> {
		let channels = usize::from(image.color().channel_count());
		image.as_bytes().chunks(channels).map(raw).collect()
	}

	/// 8×8, the left three columns one step below a byte boundary, the rest
	/// one step above it: `(0, 0, 255)` next to `(0, 1, 0)`.
	fn step_image() -> DynamicImage {
		DynamicImage::from_fn(8, 8, |x, _| if x < 3 { [0u8, 0, 255] } else { [0u8, 1, 0] })
	}

	/// Two levels above 0/0/0, the tile 2/1/0 holds its columns 2 and 3: one
	/// on each side of the step.
	const ACROSS_THE_STEP: (u8, u32, u32) = (2, 1, 0);

	#[test]
	fn elevation_stays_between_the_heights_it_is_interpolated_from() -> Result<()> {
		let (level, x, y) = ACROSS_THE_STEP;
		let coord_src = TileCoord::new(0, 0, 0)?;
		let coord_dst = TileCoord::new(level, x, y)?;

		let scaled = heights(&Scaling::Elevation.extract(&step_image(), coord_src, coord_dst)?);
		assert!(scaled.iter().all(|h| (255..=256).contains(h)), "{scaled:?}");
		assert!(scaled.contains(&255) && scaled.contains(&256));
		// Rising from left to right, in every row.
		assert!(scaled.chunks(8).all(<[u32]>::is_sorted), "{scaled:?}");

		// The same tile, scaled as colors: heights far from both sides.
		let as_color = heights(&Scaling::Color.extract(&step_image(), coord_src, coord_dst)?);
		assert!(as_color.iter().any(|h| !(255..=256).contains(h)), "{as_color:?}");
		Ok(())
	}

	#[test]
	fn elevation_of_a_flat_tile_stays_flat() -> Result<()> {
		let image = DynamicImage::from_fn(8, 8, |_, _| [1u8, 134, 160]);
		for (level, x, y) in [(0, 0, 0), (1, 1, 1), (3, 5, 2), (9, 300, 77)] {
			let scaled = Scaling::Elevation.extract(&image, TileCoord::new(0, 0, 0)?, TileCoord::new(level, x, y)?)?;
			assert!(heights(&scaled).iter().all(|h| *h == 100_000), "{level}/{x}/{y}");
		}
		Ok(())
	}

	/// The left three columns have no data. Its neighbours must not be pulled toward
	/// zero by it, and it must not fill up from them.
	#[test]
	fn elevation_ignores_pixels_without_data() -> Result<()> {
		let image = DynamicImage::from_fn(8, 8, |x, _| if x < 3 { [0u8, 0, 0, 0] } else { [1u8, 134, 160, 255] });
		let (level, x, y) = ACROSS_THE_STEP;
		let scaled = Scaling::Elevation.extract(&image, TileCoord::new(0, 0, 0)?, TileCoord::new(level, x, y)?)?;

		for row in scaled.as_bytes().chunks(4 * 8) {
			let pixels: Vec<&[u8]> = row.chunks(4).collect();
			// Half the row has no data, the other half the one height there is.
			assert!(pixels[..4].iter().all(|p| p[3] == 0), "{pixels:?}");
			assert!(
				pixels[4..].iter().all(|p| p[3] == 255 && raw(p) == 100_000),
				"{pixels:?}"
			);
		}
		Ok(())
	}

	#[test]
	fn elevation_needs_eight_bit_rgb() {
		let image = DynamicImage::from_fn(8, 8, |x, _| [x as u8]);
		let coord = TileCoord::new(0, 0, 0).unwrap();
		assert!(Scaling::Elevation.extract(&image, coord, coord).is_err());
	}

	#[rstest::rstest]
	#[case("dem/mapbox", Some(Scaling::Elevation))]
	#[case("dem/terrarium", Some(Scaling::Elevation))]
	#[case("rgb", Some(Scaling::Color))]
	#[case("rgba", Some(Scaling::Color))]
	#[case("dem/versatiles", None)]
	fn the_schema_decides_how_tiles_are_scaled(#[case] schema: &str, #[case] expected: Option<Scaling>) {
		let tilejson = TileJSON {
			tile_schema: Some(TileSchema::try_from(schema).unwrap()),
			..TileJSON::default()
		};
		assert_eq!(Scaling::for_source(&tilejson).ok(), expected);
		assert_eq!(Scaling::for_source(&TileJSON::default()).ok(), Some(Scaling::Color));
	}

	/// Elevation tiles are marked lossless; a lossy WebP would change heights.
	#[test]
	fn an_elevation_tile_is_encoded_without_loss() -> Result<()> {
		use versatiles_core::TileCompression::Uncompressed;

		let image = DynamicImage::from_fn(64, 64, |x, y| [1u8, (x * 3 + y) as u8, (x * 37 + y * 11) as u8]);
		let coord_src = TileCoord::new(0, 0, 0)?;
		let coord_dst = TileCoord::new(1, 1, 0)?;

		let scaled = Scaling::Elevation.extract(&image, coord_src, coord_dst)?;
		let tile = Scaling::Elevation.tile(&image, coord_src, coord_dst, TileFormat::WEBP)?;
		let blob = tile.into_blob(&Uncompressed)?;
		assert_eq!(
			DynamicImage::from_blob(&blob, TileFormat::WEBP)?.as_bytes(),
			scaled.as_bytes()
		);
		Ok(())
	}

	// --- color ---

	fn gradient(channel_count: usize) -> DynamicImage {
		let s = 256;
		match channel_count {
			1 => DynamicImage::from_fn(s, s, |x, _| [x as u8]),
			2 => DynamicImage::from_fn(s, s, |x, y| [x as u8, y as u8]),
			3 => DynamicImage::from_fn(s, s, |x, y| [x as u8, y as u8, 255 - x as u8]),
			4 => DynamicImage::from_fn(s, s, |x, y| [x as u8, y as u8, 255 - x as u8, 255 - y as u8]),
			_ => panic!("unsupported channel count {channel_count}"),
		}
	}

	/// The average color of the area of `dst` cut out of a gradient at `src`,
	/// divided by `scale` to make the expected values readable.
	#[expect(
		clippy::cast_sign_loss,
		reason = "an average of u8 values divided by a positive number"
	)]
	fn average(channel_count: usize, src: (u8, u32, u32), dst: (u8, u32, u32), scale: u32) -> Vec<u8> {
		let image = Scaling::Color
			.extract(
				&gradient(channel_count),
				TileCoord::new(src.0, src.1, src.2).unwrap(),
				TileCoord::new(dst.0, dst.1, dst.2).unwrap(),
			)
			.unwrap();
		image
			.average_color()
			.into_iter()
			.map(|c| (f64::from(c) / f64::from(scale)).round() as u8)
			.collect()
	}

	/// One level up, a tile is a quarter of its source: each quarter of the
	/// gradient comes out with its own average, in every channel layout.
	#[rstest::rstest]
	#[case::l(1, [vec![16], vec![48], vec![16], vec![48]])]
	#[case::la(2, [vec![16, 16], vec![48, 16], vec![16, 48], vec![48, 48]])]
	#[case::rgb(3, [vec![16, 16, 48], vec![48, 16, 16], vec![16, 48, 48], vec![48, 48, 16]])]
	#[case::rgba(4, [vec![16, 16, 48, 48], vec![48, 16, 16, 48], vec![16, 48, 48, 16], vec![48, 48, 16, 16]])]
	fn color_one_level_up_is_a_quarter_of_the_source(#[case] channel_count: usize, #[case] expected: [Vec<u8>; 4]) {
		let src = (2, 1, 1);
		let quarters = [(3, 2, 2), (3, 3, 2), (3, 2, 3), (3, 3, 3)];
		assert_eq!(quarters.map(|dst| average(channel_count, src, dst, 4)), expected);
	}

	#[test]
	fn color_two_levels_up_is_a_sixteenth_of_the_source() {
		for x in 0..4u32 {
			for y in 0..4u32 {
				assert_eq!(
					average(3, (2, 1, 1), (4, 4 + x, 4 + y), 32),
					vec![(1 + x * 2) as u8, (1 + y * 2) as u8, (7 - x * 2) as u8]
				);
			}
		}
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
