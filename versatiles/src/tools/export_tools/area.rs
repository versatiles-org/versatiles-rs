//! `versatiles export area`: the raster tiles of a bbox as one image.

use std::{f64::consts::PI, path::PathBuf};

use anyhow::{Context, Result, bail, ensure};
use versatiles_container::TilesRuntime;
use versatiles_core::{GeoBBox, TileBBox, TileFormat, TileType};
use versatiles_image::{
	DynamicImage, DynamicImageTraitConvert, DynamicImageTraitInfo, GenericImage, GenericImageView, ImageBuffer, Rgba,
};

use super::output;
use crate::tools::convert::parse_bbox;

/// Half the side of the Web Mercator square, in metres.
const MERCATOR_EXTENT: f64 = 20_037_508.342_789_244;

/// The latitude where the Web Mercator square ends.
const MAX_LATITUDE: f64 = 85.051_128_779_806_59;

/// The longest side WebP can encode.
const WEBP_MAX_SIDE: u64 = 16_383;

#[derive(clap::Args, Debug)]
#[command(
	arg_required_else_help = true,
	disable_version_flag = true,
	about = "Export the raster tiles of an area as one image",
	long_about = "\
Export the raster tiles of an area as one image.

Reads the tiles at --zoom that cover --bbox, puts them together, crops the
result to the bbox and encodes it in the format the extension of the output
file names: .png, .webp or .jpg. The image stays in Web Mercator, at the
resolution of the zoom level.

Next to the image goes a world file (.pgw, .wpw or .jgw) that places it in
EPSG:3857, so GIS software shows it where it belongs.

Missing tiles are left transparent. JPEG has no transparency, so an area
with missing or translucent tiles has to be exported as PNG or WebP.

An image larger than --max-pixels is refused rather than scaled down:
choose a lower --zoom, a smaller bbox, or a higher --max-pixels.

EXAMPLES

  # Central Berlin at zoom 16
  versatiles export area satellite.versatiles --bbox 13.37,52.50,13.42,52.53 --zoom 16 -o berlin.webp"
)]
pub struct Area {
	/// Tile container to read (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(value_name = "INPUT_FILE", verbatim_doc_comment)]
	input: String,

	/// The area to export, in degrees.
	#[arg(long, value_name = "lon_min,lat_min,lon_max,lat_max", allow_hyphen_values = true)]
	bbox: String,

	/// The zoom level whose tiles to export, and so the resolution of the image.
	#[arg(long, value_name = "int")]
	zoom: u8,

	/// Image file to write: .png, .webp or .jpg.
	#[arg(short, long, value_name = "FILE")]
	output: PathBuf,

	/// Encoder quality for .webp and .jpg, 0 to 100. Defaults to the encoder's own.
	#[arg(long, value_name = "int", value_parser = clap::value_parser!(u8).range(0..=100))]
	quality: Option<u8>,

	/// The largest image to write, in pixels.
	#[arg(long, value_name = "int", default_value_t = 64_000_000)]
	max_pixels: u64,
}

pub async fn run(args: &Area, runtime: &TilesRuntime) -> Result<()> {
	let bbox = parse_bbox(&args.bbox)?;
	let format = image_format(args)?;

	let reader = runtime
		.reader_from_str(&args.input)
		.await
		.with_context(|| format!("opening {}", args.input))?;
	let tile_format = *reader.metadata().tile_format();
	ensure!(
		tile_format.to_type() == TileType::Raster,
		"export area writes raster tiles as an image, and {} holds {} tiles",
		args.input,
		tile_format.as_str()
	);

	// Which tiles: the same for any tile size, so it is known before a tile is read.
	let tiles = Crop::new(&bbox, args.zoom, 1);
	let tile_bbox = TileBBox::from_min_and_max(
		args.zoom,
		u32::try_from(tiles.x0)?,
		u32::try_from(tiles.y0)?,
		u32::try_from(tiles.x0 + tiles.width - 1)?,
		u32::try_from(tiles.y0 + tiles.height - 1)?,
	)?;

	// Refuse an area that is too large before reading it, going by the tile
	// size the container declares, or the smallest there is.
	let declared_size = reader.tilejson().tile_size.map_or(256, |size| u64::from(size.size()));
	check_size(&Crop::new(&bbox, args.zoom, declared_size), args, format)?;

	let tiles = reader.tile_stream(tile_bbox).await?.to_vec().await;
	ensure!(
		!tiles.is_empty(),
		"{} has no tiles in this area at zoom {}",
		args.input,
		args.zoom
	);
	let missing = tile_bbox.count_tiles() - tiles.len() as u64;
	if missing > 0 {
		log::warn!(
			"{missing} of {} tiles are missing and left transparent",
			tile_bbox.count_tiles()
		);
	}

	let mut images = Vec::with_capacity(tiles.len());
	for (coord, tile) in tiles {
		let image = tile.into_image()?;
		ensure!(
			image.width() == image.height(),
			"tile {}/{}/{} is {}x{}, and only square tiles fit together",
			coord.level,
			coord.x,
			coord.y,
			image.width(),
			image.height()
		);
		images.push((coord, image));
	}
	let tile_size = images[0].1.width();
	ensure!(
		images.iter().all(|(_, image)| image.width() == tile_size),
		"the tiles of this area differ in size"
	);

	let crop = Crop::new(&bbox, args.zoom, u64::from(tile_size));
	check_size(&crop, args, format)?;
	let image = crop.compose(&images, tile_size)?;

	ensure!(
		format != TileFormat::JPG || image.is_opaque(),
		"the area has missing or translucent tiles, and JPEG has no transparency: \
		 export it as .png or .webp"
	);
	ensure!(
		format != TileFormat::PNG || args.quality.is_none(),
		"PNG is lossless, so --quality applies only to .webp and .jpg"
	);
	let blob = image.to_blob(format, args.quality, None)?;
	output::write(blob.as_slice(), Some(&args.output))?;

	let world_file = world_file_path(&args.output, format);
	output::write(crop.world_file().as_bytes(), Some(&world_file))?;
	log::info!(
		"exported {}x{} pixels to {} and {}",
		crop.width,
		crop.height,
		args.output.display(),
		world_file.display()
	);
	Ok(())
}

/// The image format the output file names.
fn image_format(args: &Area) -> Result<TileFormat> {
	let name = args.output.to_string_lossy().to_string();
	let mut stem = name.clone();
	match TileFormat::from_filename(&mut stem) {
		Some(format @ (TileFormat::PNG | TileFormat::WEBP | TileFormat::JPG)) => Ok(format),
		_ => bail!("'{name}' has to end in .png, .webp or .jpg, which name the image format to write"),
	}
}

/// Refuses an image larger than allowed, or than its format can hold.
fn check_size(crop: &Crop, args: &Area, format: TileFormat) -> Result<()> {
	let pixels = crop.width * crop.height;
	ensure!(
		pixels <= args.max_pixels,
		"the image would be {}x{} = {pixels} pixels, more than --max-pixels {}: \
		 choose a lower --zoom, a smaller --bbox, or a higher --max-pixels",
		crop.width,
		crop.height,
		args.max_pixels
	);
	ensure!(
		format != TileFormat::WEBP || (crop.width <= WEBP_MAX_SIDE && crop.height <= WEBP_MAX_SIDE),
		"the image would be {}x{} pixels, and WebP holds at most {WEBP_MAX_SIDE} per side: \
		 export it as .png, or choose a lower --zoom or a smaller --bbox",
		crop.width,
		crop.height
	);
	Ok(())
}

/// The world file next to an image: `.pgw` for `.png`, and so on.
fn world_file_path(image: &std::path::Path, format: TileFormat) -> PathBuf {
	let extension = match format {
		TileFormat::PNG => "pgw",
		TileFormat::JPG => "jgw",
		_ => "wpw",
	};
	image.with_extension(extension)
}

/// A rectangle of pixels in the Web Mercator square at one zoom level, with
/// `tile_size` pixels per tile: the pixels of a bbox, rounded outwards.
#[derive(Debug, PartialEq)]
struct Crop {
	/// The column of the left edge.
	x0: u64,
	/// The row of the top edge.
	y0: u64,
	width: u64,
	height: u64,
	/// Pixels across the whole square.
	world: u64,
}

impl Crop {
	/// With `tile_size` 1, the pixels are the tiles that cover the bbox.
	fn new(bbox: &GeoBBox, zoom: u8, tile_size: u64) -> Self {
		let world = tile_size << zoom;
		#[expect(clippy::cast_precision_loss, reason = "at most 2^31 · 512, exact in f64")]
		let size = world as f64;
		#[expect(
			clippy::cast_possible_truncation,
			clippy::cast_sign_loss,
			reason = "a fraction of the square, rounded, so in 0..=world"
		)]
		let edge = |fraction: f64, round: fn(f64) -> f64| round(fraction * size) as u64;
		let (west, north) = world_fraction(bbox.x_min, bbox.y_max);
		let (east, south) = world_fraction(bbox.x_max, bbox.y_min);
		let x0 = edge(west, f64::floor).min(world - 1);
		let y0 = edge(north, f64::floor).min(world - 1);
		// At least one pixel, so that a point is the pixel it lies in.
		let x1 = edge(east, f64::ceil).clamp(x0 + 1, world);
		let y1 = edge(south, f64::ceil).clamp(y0 + 1, world);
		Crop {
			x0,
			y0,
			width: x1 - x0,
			height: y1 - y0,
			world,
		}
	}

	/// The tiles put together and cropped. Pixels no tile covers stay transparent.
	fn compose(&self, tiles: &[(versatiles_core::TileCoord, DynamicImage)], tile_size: u32) -> Result<DynamicImage> {
		let mut image = ImageBuffer::from_pixel(
			u32::try_from(self.width)?,
			u32::try_from(self.height)?,
			Rgba([0, 0, 0, 0]),
		);
		let size = u64::from(tile_size);
		for (coord, tile) in tiles {
			let tile = tile.to_rgba8();
			// The tile's pixels, and the part of them inside the crop.
			let (left, top) = (u64::from(coord.x) * size, u64::from(coord.y) * size);
			let x_from = left.max(self.x0);
			let y_from = top.max(self.y0);
			let x_to = (left + size).min(self.x0 + self.width);
			let y_to = (top + size).min(self.y0 + self.height);
			if x_from >= x_to || y_from >= y_to {
				continue;
			}
			let part = tile.view(
				u32::try_from(x_from - left)?,
				u32::try_from(y_from - top)?,
				u32::try_from(x_to - x_from)?,
				u32::try_from(y_to - y_from)?,
			);
			image.copy_from(
				&*part,
				u32::try_from(x_from - self.x0)?,
				u32::try_from(y_from - self.y0)?,
			)?;
		}
		Ok(DynamicImage::ImageRgba8(image))
	}

	/// The six lines of a world file placing the image in EPSG:3857: the
	/// pixel size, two rotations, the negative pixel height, and the centre
	/// of the top left pixel.
	fn world_file(&self) -> String {
		#[expect(clippy::cast_precision_loss, reason = "pixel counts far below 2^52")]
		let (world, x0, y0) = (self.world as f64, self.x0 as f64, self.y0 as f64);
		let pixel = 2.0 * MERCATOR_EXTENT / world;
		let x = -MERCATOR_EXTENT + (x0 + 0.5) * pixel;
		let y = MERCATOR_EXTENT - (y0 + 0.5) * pixel;
		format!("{pixel:.10}\n0.0\n0.0\n{:.10}\n{x:.10}\n{y:.10}\n", -pixel)
	}
}

/// Where a point lies in the Web Mercator square, as fractions from its west
/// and north edges.
fn world_fraction(lon: f64, lat: f64) -> (f64, f64) {
	let lat = lat.clamp(-MAX_LATITUDE, MAX_LATITUDE).to_radians();
	let x = (lon + 180.0) / 360.0;
	let y = 0.5 - (PI / 4.0 + lat / 2.0).tan().ln() / (2.0 * PI);
	(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
	use versatiles_core::TileCoord;

	use super::*;

	fn bbox(x_min: f64, y_min: f64, x_max: f64, y_max: f64) -> GeoBBox {
		GeoBBox::new(x_min, y_min, x_max, y_max).unwrap()
	}

	#[test]
	fn the_whole_world_is_every_pixel() {
		let crop = Crop::new(&bbox(-180.0, -85.1, 180.0, 85.1), 1, 256);
		assert_eq!((crop.x0, crop.y0, crop.width, crop.height), (0, 0, 512, 512));
	}

	#[test]
	fn a_bbox_is_rounded_outwards_to_whole_pixels() {
		// The north-east quarter, nudged inwards: one tile at zoom 1.
		let crop = Crop::new(&bbox(0.001, 0.001, 179.999, 85.0), 1, 1);
		assert_eq!((crop.x0, crop.y0, crop.width, crop.height), (1, 0, 1, 1));
		// The same quarter in pixels.
		let crop = Crop::new(&bbox(0.0, 0.0, 180.0, 85.1), 1, 256);
		assert_eq!((crop.x0, crop.y0, crop.width, crop.height), (256, 0, 256, 256));
	}

	#[test]
	fn a_point_is_the_pixel_it_lies_in() {
		let crop = Crop::new(&bbox(13.40, 52.52, 13.40, 52.52), 14, 1);
		assert_eq!((crop.x0, crop.y0, crop.width, crop.height), (8801, 5373, 1, 1));
		assert_eq!(
			TileCoord::from_geo(13.40, 52.52, 14).unwrap(),
			TileCoord::new(14, 8801, 5373).unwrap()
		);
	}

	#[test]
	fn the_world_file_places_the_top_left_pixel() {
		let crop = Crop::new(&bbox(-180.0, -85.1, 180.0, 85.1), 0, 256);
		let lines: Vec<f64> = crop.world_file().lines().map(|line| line.parse().unwrap()).collect();
		let pixel = 2.0 * MERCATOR_EXTENT / 256.0;
		assert!((lines[0] - pixel).abs() < 1e-6);
		assert!((lines[3] + pixel).abs() < 1e-6);
		assert!((lines[4] - (-MERCATOR_EXTENT + pixel / 2.0)).abs() < 1e-6);
		assert!((lines[5] - (MERCATOR_EXTENT - pixel / 2.0)).abs() < 1e-6);
	}

	/// Four 2×2 tiles of one colour each, cropped to the middle 2×2 pixels.
	#[test]
	fn tiles_are_put_together_and_cropped() -> Result<()> {
		let colour = |value: u8| DynamicImage::ImageRgba8(ImageBuffer::from_pixel(2, 2, Rgba([value, 0, 0, 255])));
		let tiles = vec![
			(TileCoord::new(1, 0, 0)?, colour(10)),
			(TileCoord::new(1, 1, 0)?, colour(20)),
			(TileCoord::new(1, 0, 1)?, colour(30)),
		];
		let crop = Crop {
			x0: 1,
			y0: 1,
			width: 2,
			height: 2,
			world: 4,
		};
		let image = crop.compose(&tiles, 2)?.to_rgba8();
		assert_eq!(image.get_pixel(0, 0).0, [10, 0, 0, 255]);
		assert_eq!(image.get_pixel(1, 0).0, [20, 0, 0, 255]);
		assert_eq!(image.get_pixel(0, 1).0, [30, 0, 0, 255]);
		assert_eq!(
			image.get_pixel(1, 1).0,
			[0, 0, 0, 0],
			"a missing tile stays transparent"
		);
		Ok(())
	}

	#[test]
	fn the_world_file_is_named_after_the_image() {
		let path = std::path::Path::new("dir/berlin.webp");
		assert_eq!(world_file_path(path, TileFormat::WEBP), PathBuf::from("dir/berlin.wpw"));
		assert_eq!(world_file_path(path, TileFormat::PNG), PathBuf::from("dir/berlin.pgw"));
		assert_eq!(world_file_path(path, TileFormat::JPG), PathBuf::from("dir/berlin.jgw"));
	}
}
