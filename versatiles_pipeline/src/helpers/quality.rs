//! Encoder quality per zoom level, optionally following a quality map.
//!
//! Shared by `raster_format` and `versatiles mosaic assemble`, so that both
//! read the same notation and resolve it the same way.

use std::{io::Cursor, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use imageproc::image::{ColorType, ImageDecoder, Limits, codecs::png::PngDecoder};
use versatiles_core::TileCoord;
use versatiles_derive::context;

/// An encoder quality setting resolved per zoom level.
///
/// Carries the format of `quality=` in its type, so that `70,14:50,15:20` is
/// judged where a value is decoded rather than by whatever parses it later,
/// including by `check`, which never builds anything (#257).
///
/// A level may hold a range such as `50-30` instead of a number: the quality
/// where a [`QualityMap`] is 0, and where it is 255.
///
/// `Default` is "nothing set at any zoom", which is what an absent `quality=`
/// means and what the encoder's own default fills in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QualityByZoom([Option<QualityRange>; 32]);

/// The quality where the map is 0 and where it is 255. Equal for a plain number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct QualityRange {
	at_0: u8,
	at_255: u8,
}

impl QualityRange {
	/// The quality for a map value, interpolated linearly and rounded.
	fn at(self, value: u8) -> u8 {
		let v = u32::from(value);
		let q = (u32::from(self.at_0) * (255 - v) + u32::from(self.at_255) * v + 127) / 255;
		u8::try_from(q).expect("an interpolation lies between its ends, which are at most 100")
	}

	fn is_range(self) -> bool {
		self.at_0 != self.at_255
	}
}

impl QualityByZoom {
	/// Whether any zoom level holds a range, which needs a [`QualityMap`].
	#[must_use]
	pub fn uses_map(&self) -> bool {
		self.0.iter().flatten().any(|range| range.is_range())
	}

	/// Whether every zoom level has a quality.
	#[must_use]
	pub fn is_set_at_every_level(&self) -> bool {
		self.0.iter().all(Option::is_some)
	}

	/// Whether `quality` is one this setting can give at some tile.
	#[must_use]
	pub fn can_give(&self, quality: u8) -> bool {
		self.0.iter().flatten().any(|range| {
			let (low, high) = (range.at_0.min(range.at_255), range.at_0.max(range.at_255));
			(low..=high).contains(&quality)
		})
	}
}

impl TryFrom<&str> for QualityByZoom {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self> {
		parse_quality(value).map(Self)
	}
}

#[context("Parsing quality string")]
fn parse_quality(text: &str) -> Result<[Option<QualityRange>; 32]> {
	let mut result: [Option<QualityRange>; 32] = [None; 32];
	let mut zoom: i32 = -1;
	for part in text.split(',') {
		let mut part = part.trim();
		zoom += 1;
		if part.is_empty() {
			continue;
		}
		if let Some(idx) = part.find(':') {
			zoom = part[0..idx].trim().parse()?;
			ensure!(
				(0..=31).contains(&zoom),
				"zoom level must be between 0 and 31, but is {zoom}"
			);
			part = &part[(idx + 1)..];
		}
		// A plain number is a range of one value.
		let (at_0, at_255) = part.split_once('-').unwrap_or((part, part));
		let range = QualityRange {
			at_0: parse_value(at_0)?,
			at_255: parse_value(at_255)?,
		};
		for z in zoom..32 {
			result[usize::try_from(z).expect("zoom in 0..32 fits in usize")] = Some(range);
		}
	}
	Ok(result)
}

fn parse_value(text: &str) -> Result<u8> {
	let text = text.trim();
	let value: u8 = text
		.parse()
		.with_context(|| format!("'{text}' is not a quality between 0 and 100"))?;
	ensure!(value <= 100, "quality must be between 0 and 100, but is {value}");
	Ok(value)
}

/// A grayscale image covering the Web Mercator world, whose pixels choose a
/// quality from the ranges of a [`QualityByZoom`].
///
/// The image is square, its side a power of two, `2^n`, so that a pixel is a
/// tile at zoom `n`, with row 0 in the north. A tile at zoom `n` or finer takes
/// the pixel it lies in. A coarser tile covers several pixels and takes
/// whichever of them gives the highest quality, so that a tile is encoded for
/// the most demanding place in it. For that, the lowest and the highest pixel
/// under every coarser tile are worked out once, when the map is read.
pub struct QualityMap {
	/// `n`: the zoom level whose tiles are the pixels.
	level: u8,
	/// The pixels, row by row.
	pixels: Vec<u8>,
	/// For each zoom level below `level`, by zoom: the lowest and the highest
	/// pixel each tile covers, row by row.
	overviews: Vec<(Vec<u8>, Vec<u8>)>,
}

impl std::fmt::Debug for QualityMap {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let side = 1u64 << self.level;
		write!(f, "QualityMap({side}x{side})")
	}
}

impl QualityMap {
	/// Reads the map from an 8-bit grayscale PNG.
	///
	/// # Errors
	///
	/// Errors when the bytes are not a PNG, when it is not 8-bit grayscale, or
	/// when it is not square with a power of two as its side.
	pub fn from_png(bytes: &[u8]) -> Result<Self> {
		let decoder = PngDecoder::with_limits(Cursor::new(bytes), Limits::default()).context("reading the PNG header")?;
		let (width, height) = decoder.dimensions();
		let color = decoder.color_type();
		ensure!(
			color == ColorType::L8,
			"a quality map has to be an 8-bit grayscale PNG, but this one is {color:?}"
		);
		ensure!(
			width == height,
			"a quality map has to be square, but this one is {width}x{height}"
		);
		let mut pixels = vec![0; usize::try_from(decoder.total_bytes())?];
		decoder.read_image(&mut pixels).context("decoding the PNG")?;
		Self::from_pixels(width, pixels)
	}

	/// The map from its pixels, row by row, `side` by `side`.
	///
	/// # Errors
	///
	/// Errors when `side` is not a power of two, or when there are not `side²` pixels.
	pub fn from_pixels(side: u32, pixels: Vec<u8>) -> Result<Self> {
		if !side.is_power_of_two() {
			bail!("the side of a quality map has to be a power of two, such as 1024 or 16384, but is {side}");
		}
		let side = usize::try_from(side)?;
		ensure!(
			pixels.len() == side * side,
			"a quality map of {side}x{side} needs {} pixels, but has {}",
			side * side,
			pixels.len()
		);
		let level = u8::try_from(side.trailing_zeros())?;

		let mut overviews: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(usize::from(level));
		let mut finer_side = side;
		while finer_side > 1 {
			let (finer_min, finer_max) = overviews.last().map_or((&pixels, &pixels), |(min, max)| (min, max));
			let coarser = halve(finer_min, finer_max, finer_side);
			overviews.push(coarser);
			finer_side /= 2;
		}
		overviews.reverse();
		Ok(Self {
			level,
			pixels,
			overviews,
		})
	}

	/// The lowest and the highest pixel under a tile.
	fn value_range(&self, coord: &TileCoord) -> (u8, u8) {
		let (x, y) = (coord.x as usize, coord.y as usize);
		if coord.level >= self.level {
			let shift = coord.level - self.level;
			let side = 1usize << self.level;
			let value = self.pixels[(y >> shift) * side + (x >> shift)];
			(value, value)
		} else {
			let (min, max) = &self.overviews[usize::from(coord.level)];
			let index = y * (1usize << coord.level) + x;
			(min[index], max[index])
		}
	}
}

/// The lowest and the highest of each 2×2 block of a `side`×`side` level.
fn halve(min: &[u8], max: &[u8], side: usize) -> (Vec<u8>, Vec<u8>) {
	let half = side / 2;
	let mut coarse_min = Vec::with_capacity(half * half);
	let mut coarse_max = Vec::with_capacity(half * half);
	for y in 0..half {
		let top = 2 * y * side;
		let bottom = top + side;
		for x in 0..half {
			let cells = [top + 2 * x, top + 2 * x + 1, bottom + 2 * x, bottom + 2 * x + 1];
			coarse_min.push(cells.iter().map(|&i| min[i]).min().expect("four cells"));
			coarse_max.push(cells.iter().map(|&i| max[i]).max().expect("four cells"));
		}
	}
	(coarse_min, coarse_max)
}

/// A [`QualityByZoom`] together with the map its ranges read, if any: the
/// quality of each tile.
#[derive(Clone, Debug, Default)]
pub struct TileQuality {
	levels: QualityByZoom,
	map: Option<Arc<QualityMap>>,
}

impl TileQuality {
	/// Joins a setting with the map its ranges read.
	///
	/// # Errors
	///
	/// Errors when the setting has a range but there is no map.
	pub fn new(levels: QualityByZoom, map: Option<Arc<QualityMap>>) -> Result<Self> {
		ensure!(
			map.is_some() || !levels.uses_map(),
			"a quality range such as '50-30' needs a quality map to choose a value in it"
		);
		Ok(Self { levels, map })
	}

	/// The quality per zoom level, as set.
	#[must_use]
	pub fn levels(&self) -> &QualityByZoom {
		&self.levels
	}

	/// The quality for a tile, or `None` where the setting names none.
	///
	/// A range gives the highest quality any pixel under the tile gives.
	#[must_use]
	pub fn at(&self, coord: &TileCoord) -> Option<u8> {
		let range = self.levels.0[usize::from(coord.level)]?;
		if !range.is_range() {
			return Some(range.at_0);
		}
		let Some(map) = &self.map else {
			unreachable!("`new` refuses a range without a map");
		};
		let (lowest, highest) = map.value_range(coord);
		let value = if range.at_255 > range.at_0 { highest } else { lowest };
		Some(range.at(value))
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	fn levels(text: &str) -> QualityByZoom {
		QualityByZoom::try_from(text).unwrap()
	}

	#[rstest]
	#[case("80 -> 80,80,80,80,80,80,80,80,80,80,80,80,80,80,80,80")]
	#[case("80,70 -> 80,70,70,70,70,70,70,70,70,70,70,70,70,70,70,70")]
	#[case("10:30 -> ,,,,,,,,,,30,30,30,30,30,30")]
	#[case("80,70,14:50,15:20 -> 80,70,70,70,70,70,70,70,70,70,70,70,70,70,50,20")]
	#[case("70,14:50-30 -> 70,70,70,70,70,70,70,70,70,70,70,70,70,70,50-30,50-30")]
	#[case("10-90, 2: 5 - 6 -> 10-90,10-90,5-6,5-6,5-6,5-6,5-6,5-6,5-6,5-6,5-6,5-6,5-6,5-6,5-6,5-6")]
	#[case(" -> ,,,,,,,,,,,,,,,")]
	#[case(", , -> ,,,,,,,,,,,,,,,")]
	#[case(" ,80 , ,  -> ,80,80,80,80,80,80,80,80,80,80,80,80,80,80,80")]
	fn parse_quality_cases(#[case] case: &str) -> Result<()> {
		let (input_str, expected_str) = case.split_once(" -> ").unwrap();
		let result = parse_quality(input_str)?;
		let result_str = result[0..16]
			.iter()
			.map(|x| match x {
				None => String::new(),
				Some(r) if r.is_range() => format!("{}-{}", r.at_0, r.at_255),
				Some(r) => r.at_0.to_string(),
			})
			.collect::<Vec<String>>()
			.join(",");
		assert_eq!(result_str, expected_str);
		Ok(())
	}

	#[rstest]
	#[case("32:10", "zoom level must be between 0 and 31, but is 32")]
	#[case("5:101", "quality must be between 0 and 100, but is 101")]
	#[case("50-101", "quality must be between 0 and 100, but is 101")]
	#[case("-30", "'' is not a quality")]
	#[case("50-", "'' is not a quality")]
	#[case("50-30-10", "'30-10' is not a quality")]
	#[case("foo", "'foo' is not a quality")]
	fn parse_quality_errors(#[case] input: &str, #[case] needle: &str) {
		let msg = format!("{:#}", parse_quality(input).unwrap_err());
		assert!(msg.contains(needle), "error '{msg}' should contain '{needle}'");
	}

	#[rstest]
	#[case("a:b")]
	#[case("5:x")]
	fn parse_quality_non_numeric_errors(#[case] input: &str) {
		assert!(parse_quality(input).is_err());
	}

	#[test]
	fn what_a_setting_can_give() {
		assert!(!levels("70,14:50").uses_map());
		assert!(!levels("70,14:50-50").uses_map(), "a range of one value is a number");
		assert!(levels("70,14:50-30").uses_map());

		assert!(levels("70,14:50").is_set_at_every_level());
		assert!(!levels("14:50").is_set_at_every_level());

		assert!(levels("90-100").can_give(100));
		assert!(levels("90-100").can_give(95));
		assert!(!levels("90-99").can_give(100));
	}

	#[rstest]
	#[case(0, 50)]
	#[case(255, 30)]
	#[case(128, 40)]
	#[case(127, 40)]
	#[case(64, 45)]
	fn a_range_is_interpolated_and_rounded(#[case] value: u8, #[case] expected: u8) {
		let range = QualityRange { at_0: 50, at_255: 30 };
		assert_eq!(range.at(value), expected);
	}

	/// 4×4, a pixel per tile at zoom 2: the north-west quarter 0 except one
	/// pixel of 255, the rest a gradient.
	fn map() -> QualityMap {
		#[rustfmt::skip]
		let pixels = vec![
			0,   0,  10,  20,
			0, 255,  30,  40,
			50, 60,  70,  80,
			90, 100, 110, 120,
		];
		QualityMap::from_pixels(4, pixels).unwrap()
	}

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	#[test]
	fn a_map_knows_the_lowest_and_highest_pixel_under_each_tile() {
		let map = map();
		assert_eq!(map.value_range(&coord(0, 0, 0)), (0, 255));
		assert_eq!(map.value_range(&coord(1, 0, 0)), (0, 255));
		assert_eq!(map.value_range(&coord(1, 1, 0)), (10, 40));
		assert_eq!(map.value_range(&coord(1, 0, 1)), (50, 100));
		assert_eq!(map.value_range(&coord(2, 3, 2)), (80, 80));
		// Finer than the map: the pixel the tile lies in.
		assert_eq!(map.value_range(&coord(3, 2, 2)), (255, 255));
		assert_eq!(map.value_range(&coord(4, 15, 0)), (20, 20));
	}

	/// A tile is encoded for the most demanding place in it, whichever way
	/// the range runs.
	#[test]
	fn a_tile_takes_the_highest_quality_any_pixel_under_it_gives() -> Result<()> {
		let map = Arc::new(map());
		let rising = TileQuality::new(levels("10-90"), Some(map.clone()))?;
		let falling = TileQuality::new(levels("90-10"), Some(map))?;

		// The north-west quarter has both 0 and 255 under it.
		assert_eq!(rising.at(&coord(1, 0, 0)), Some(90));
		assert_eq!(falling.at(&coord(1, 0, 0)), Some(90));
		// The north-east quarter runs from 10 to 40.
		assert_eq!(rising.at(&coord(1, 1, 0)), Some(23));
		assert_eq!(falling.at(&coord(1, 1, 0)), Some(87));
		Ok(())
	}

	#[test]
	fn a_number_ignores_the_map_and_a_level_without_a_quality_has_none() -> Result<()> {
		let quality = TileQuality::new(levels("2:70,3:10-90"), Some(Arc::new(map())))?;
		assert_eq!(quality.at(&coord(1, 0, 0)), None);
		assert_eq!(quality.at(&coord(2, 1, 1)), Some(70));
		assert_eq!(quality.at(&coord(3, 2, 2)), Some(90));
		Ok(())
	}

	#[test]
	fn a_range_needs_a_map() {
		assert!(TileQuality::new(levels("70"), None).is_ok());
		let error = TileQuality::new(levels("70,14:50-30"), None).unwrap_err().to_string();
		assert!(error.contains("needs a quality map"), "{error}");
	}

	#[rstest]
	#[case(1, vec![7])]
	#[case(2, vec![1, 2, 3, 4])]
	fn small_maps_work(#[case] side: u32, #[case] pixels: Vec<u8>) -> Result<()> {
		let map = QualityMap::from_pixels(side, pixels)?;
		assert_eq!(map.value_range(&coord(0, 0, 0)).0, if side == 1 { 7 } else { 1 });
		Ok(())
	}

	#[rstest]
	#[case(3, 9, "power of two")]
	#[case(4, 15, "needs 16 pixels")]
	fn bad_maps_are_refused(#[case] side: u32, #[case] count: usize, #[case] needle: &str) {
		let error = QualityMap::from_pixels(side, vec![0; count]).unwrap_err().to_string();
		assert!(error.contains(needle), "{error}");
	}

	fn png(color: imageproc::image::ExtendedColorType, width: u32, height: u32, bytes: &[u8]) -> Vec<u8> {
		use imageproc::image::{ImageEncoder, codecs::png::PngEncoder};
		let mut out = Vec::new();
		PngEncoder::new(&mut out)
			.write_image(bytes, width, height, color)
			.unwrap();
		out
	}

	#[test]
	fn a_map_is_read_from_a_grayscale_png() -> Result<()> {
		use imageproc::image::ExtendedColorType::{L8, Rgb8};
		let map = QualityMap::from_png(&png(L8, 2, 2, &[1, 2, 3, 4]))?;
		assert_eq!(map.value_range(&coord(0, 0, 0)), (1, 4));
		assert_eq!(map.value_range(&coord(1, 1, 1)), (4, 4));

		let error = QualityMap::from_png(&png(Rgb8, 2, 2, &[0; 12]))
			.unwrap_err()
			.to_string();
		assert!(error.contains("8-bit grayscale"), "{error}");
		let error = QualityMap::from_png(&png(L8, 4, 2, &[0; 8])).unwrap_err().to_string();
		assert!(error.contains("square"), "{error}");
		assert!(QualityMap::from_png(b"not a png").is_err());
		Ok(())
	}
}
