use std::{f64::consts::PI, sync::Arc};

use anyhow::{Result, anyhow, bail, ensure};
use async_trait::async_trait;
use futures::{StreamExt, future::try_join_all, stream};
use imageproc::image::{DynamicImage, Rgba, RgbaImage};
use moka::future::Cache;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, TilesRuntime};
use versatiles_core::{
	ConcurrencyLimits, SharedWork, TileBBox, TileCoord, TileFormat, TileJSON, TileSchema, TileStream,
};
use versatiles_derive::context;
use versatiles_image::color::HexColor;

use super::{
	encoding::{DemEncoding, decode_elevation, resolve_encoding},
	slope::maplibre_slope_exaggeration,
};
use crate::{PipelineFactory, vpl::VPLNode};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Renders DEM tiles as hillshading, the way MapLibre's `hillshade` layer shades them.
///
/// The result is RGBA tiles that are transparent on flat ground and shade
/// slopes by their steepness and the direction they face. They are an ordinary
/// raster source for any map client, and much smaller than the elevation
/// tiles, since they can be encoded lossily with a following
/// `raster_format format=webp quality=70`.
///
/// The shading is MapLibre's default `standard` method, including the slope
/// exaggeration it applies below zoom 15 and its correction for latitude, so a
/// tile looks as a live `hillshade` layer would at the same zoom. The colours
/// are given as `RRGGBB`, `RRGGBBAA` or `[r,g,b,a]`, and an alpha below `FF`
/// makes that part of the shading lighter.
///
/// The slopes at a tile's edge are computed with the pixels of the adjacent
/// tiles, so there are no seams; the tiles are read in chunks with a one-tile
/// ring around them, and decoded heights are cached for the neighbours that
/// follow. Across the antimeridian the neighbour wraps around. Where there is
/// none, at the poles or the edge of the data, the edge pixels are repeated.
struct Args {
	/// Direction the light comes from, in degrees clockwise from north. Defaults to `335`.
	#[vpl(default = "335")]
	azimuth: Option<f64>,
	/// Intensity of the shading, from 0 to 1. Defaults to `0.5`.
	#[vpl(default = "0.5")]
	exaggeration: Option<f64>,
	/// Colour of slopes facing away from the light. Defaults to `000000`.
	#[vpl(default = "000000")]
	shadow_color: Option<HexColor>,
	/// Colour of slopes facing the light. Defaults to `FFFFFF`.
	#[vpl(default = "FFFFFF")]
	highlight_color: Option<HexColor>,
	/// Colour that accentuates steep terrain, such as cliffs and gorges. Defaults to `000000`.
	#[vpl(default = "000000")]
	accent_color: Option<HexColor>,
	/// DEM encoding of the source. Defaults to the encoding its tile schema implies.
	encoding: Option<DemEncoding>,
}

/// Width of the zoom 0 world in meters, from which the ground size of a pixel
/// follows, as in MapLibre.
const WORLD_WIDTH_METERS: f64 = 40_075_016.685_578_5;

/// Decoded heights the cache may hold, in bytes.
const CACHE_BYTES: u64 = 512 * 1024 * 1024;

/// Tiles one side of a chunk spans at 256 px: 16 × 16 tiles plus their ring
/// are 324 decoded tiles, about 85 MB. Larger tiles get smaller chunks.
const CHUNK_TILES_AT_256: u32 = 16;

/// Chunks worked on at once in a stream.
const CHUNKS_IN_FLIGHT: usize = 2;

/// A light and its colours, as MapLibre's `standard` hillshade takes them.
#[derive(Clone, Copy, Debug)]
struct Style {
	/// In radians, with MapLibre's offset of π already added.
	azimuth: f64,
	intensity: f64,
	shadow: [f64; 4],
	highlight: [f64; 4],
	accent: [f64; 4],
}

impl Style {
	/// `color` as premultiplied RGBA in 0..=1, as MapLibre passes colours to
	/// its shaders.
	fn premultiplied(color: Option<&HexColor>, default: [u8; 4]) -> [f64; 4] {
		let channels = color.map_or(default.to_vec(), |color| color.channels().to_vec());
		let [r, g, b] = [0, 1, 2].map(|i| f64::from(channels[i]) / 255.0);
		let a = channels.get(3).map_or(1.0, |&a| f64::from(a) / 255.0);
		[r * a, g * a, b * a, a]
	}
}

/// The heights of one source tile, in meters, row by row.
#[derive(Debug)]
struct Heights {
	size: u32,
	meters: Vec<f32>,
}

impl Heights {
	#[context("decoding the heights of tile {coord:?}")]
	fn decode(coord: TileCoord, tile: Tile, encoding: DemEncoding) -> Result<Heights> {
		let image = tile.into_image()?;
		let (width, height) = (image.width(), image.height());
		ensure!(width == height, "a DEM tile must be square, not {width}x{height} px");
		#[expect(
			clippy::cast_possible_truncation,
			reason = "heights fit an f32 to well below a millimetre"
		)]
		let meters = match image {
			DynamicImage::ImageRgb8(image) => image.pixels().map(|p| decode_elevation(encoding, p.0) as f32).collect(),
			DynamicImage::ImageRgba8(image) => image
				.pixels()
				.map(|p| decode_elevation(encoding, [p.0[0], p.0[1], p.0[2]]) as f32)
				.collect(),
			_ => bail!("dem_hillshade requires RGB8 or RGBA8 images"),
		};
		Ok(Heights { size: width, meters })
	}

	fn at(&self, x: u32, y: u32) -> f32 {
		self.meters[(y * self.size + x) as usize]
	}

	/// The heights with a one-pixel border taken from `neighbours`, row by row,
	/// `(size + 2)²` of them. `neighbours[dy + 1][dx + 1]` is the tile `dx`
	/// across and `dy` down; where one is missing or of another size, the edge
	/// of this tile is repeated, as MapLibre does before it can backfill.
	fn with_border(&self, neighbours: &[[Option<Arc<Heights>>; 3]; 3]) -> Vec<f32> {
		let size = self.size;
		let side = size + 2;
		// Where row or column `g` of the bordered grid comes from: which tile
		// (0 before this one, 1 this one, 2 after it), and where in that tile.
		let from = |g: u32| match g {
			0 => (0, size - 1),
			g if g == side - 1 => (2, 0),
			g => (1, g - 1),
		};
		let mut grid = Vec::with_capacity(side as usize * side as usize);
		for gy in 0..side {
			let (ty, y) = from(gy);
			for gx in 0..side {
				let (tx, x) = from(gx);
				let neighbour = if (tx, ty) == (1, 1) {
					None
				} else {
					neighbours[ty][tx].as_deref().filter(|n| n.size == size)
				};
				grid.push(match neighbour {
					Some(n) => n.at(x, y),
					// Repeat the edge: the pixel of this tile nearest to it.
					None => self.at(gx.saturating_sub(1).min(size - 1), gy.saturating_sub(1).min(size - 1)),
				});
			}
		}
		grid
	}
}

/// Shades a tile of `size` px from its heights with a border, `(size + 2)²`
/// values row by row, as MapLibre's `standard` method shades it at zoom
/// `coord.level`.
///
/// Follows `hillshade_prepare.fragment.glsl` for the slope, including the
/// clamp that storing it in 8 bits imposes there, and `standard_hillshade` in
/// `hillshade.fragment.glsl` for the colour.
#[expect(
	clippy::many_single_char_names,
	reason = "`a` to `i` are the 3×3 window as MapLibre's shader names it, which makes the two easy to compare"
)]
fn shade(grid: &[f32], size: u32, coord: &TileCoord, style: &Style) -> RgbaImage {
	let side = size as usize + 2;
	let at = |x: usize, y: usize| f64::from(grid[y * side + x]);

	let tiles = f64::from(1u32 << coord.level);
	let pixel_meters = WORLD_WIDTH_METERS / tiles / f64::from(size);
	let factor = maplibre_slope_exaggeration(coord.level) / (8.0 * pixel_meters);
	// The prepared slope is stored as `deriv / 8 + 128 / 255` in 8 bits.
	let (deriv_min, deriv_max) = (-8.0 * 128.0 / 255.0, 8.0 * 127.0 / 255.0);
	let [_, lat_top] = TileCoord::coord_to_geo(coord.level, f64::from(coord.x), f64::from(coord.y));
	let [_, lat_bottom] = TileCoord::coord_to_geo(coord.level, f64::from(coord.x), f64::from(coord.y) + 1.0);

	let strength = (style.intensity * 2.0).clamp(0.0, 1.0);
	let base = 1.875 - style.intensity * 1.75;
	let max_slope = 0.5 * PI;

	let mut image = RgbaImage::new(size, size);
	for (py, row) in image.rows_mut().enumerate() {
		let lat = lat_top + (lat_bottom - lat_top) * ((py as f64 + 0.5) / f64::from(size));
		let scale = lat.to_radians().cos();
		for (px, pixel) in row.enumerate() {
			let (x, y) = (px + 1, py + 1);
			let (a, b, c) = (at(x - 1, y - 1), at(x, y - 1), at(x + 1, y - 1));
			let (d, f) = (at(x - 1, y), at(x + 1, y));
			let (g, h, i) = (at(x - 1, y + 1), at(x, y + 1), at(x + 1, y + 1));
			let dx = (((c + 2.0 * f + i) - (a + 2.0 * d + g)) * factor).clamp(deriv_min, deriv_max) / scale;
			let dy = (((g + 2.0 * h + i) - (a + 2.0 * b + c)) * factor).clamp(deriv_min, deriv_max) / scale;

			let slope = (0.625 * dx.hypot(dy)).atan();
			let aspect = if dx == 0.0 {
				PI / 2.0 * if dy > 0.0 { 1.0 } else { -1.0 }
			} else {
				dy.atan2(-dx)
			};
			let scaled = if (style.intensity - 0.5).abs() > f64::EPSILON {
				(base.powf(slope) - 1.0) / (base.powf(max_slope) - 1.0) * max_slope
			} else {
				slope
			};
			let accent = (1.0 - scaled.cos()) * strength;
			let facing = (((aspect + style.azimuth) / PI + 0.5).rem_euclid(2.0) - 1.0).abs();
			let weight = scaled.sin() * strength;
			let shade_color: [f64; 4] =
				[0, 1, 2, 3].map(|k| (style.shadow[k] + (style.highlight[k] - style.shadow[k]) * facing) * weight);
			let color: [f64; 4] = [0, 1, 2, 3].map(|k| style.accent[k] * accent * (1.0 - shade_color[3]) + shade_color[k]);
			*pixel = unpremultiply(color);
		}
	}
	image
}

/// A premultiplied colour in 0..=1 as straight RGBA8, as images store it.
fn unpremultiply([r, g, b, a]: [f64; 4]) -> Rgba<u8> {
	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		reason = "clamped to 0..=255"
	)]
	let byte = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
	if a <= 0.0 {
		return Rgba([0, 0, 0, 0]);
	}
	Rgba([byte(r / a), byte(g / a), byte(b / a), byte(a)])
}

/// The coordinates around `coord`, `[dy + 1][dx + 1]`: wrapping around the
/// antimeridian, and `None` beyond the poles.
fn neighbour_coords(coord: &TileCoord) -> [[Option<TileCoord>; 3]; 3] {
	let count = i64::from(1u32 << coord.level);
	[-1i64, 0, 1].map(|dy| {
		[-1i64, 0, 1].map(|dx| {
			let y = i64::from(coord.y) + dy;
			let x = (i64::from(coord.x) + dx).rem_euclid(count);
			#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "within 0..count")]
			(0..count)
				.contains(&y)
				.then(|| TileCoord::new(coord.level, x as u32, y as u32).ok())
				.flatten()
		})
	})
}

#[derive(Clone)]
pub struct Operation {
	inner: Arc<Inner>,
}

struct Inner {
	source: Box<dyn TileSource>,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
	style: Style,
	encoding: DemEncoding,
	/// Decoded source tiles; `None` is a tile the source lacks.
	heights: Cache<TileCoord, Option<Arc<Heights>>>,
	/// Which requests wait for which entry of `heights` (#265).
	shared: SharedWork<TileCoord>,
	runtime: TilesRuntime,
}

impl std::fmt::Debug for Operation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("dem_hillshade")
			.field("style", &self.inner.style)
			.field("encoding", &self.inner.encoding)
			.finish_non_exhaustive()
	}
}

impl Operation {
	#[context("Building dem_hillshade operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, factory: &PipelineFactory) -> Result<Operation> {
		let args = Args::from_vpl_node(&vpl_node)?;
		let encoding = resolve_encoding(args.encoding, source.tilejson().tile_schema.as_ref())?;
		let intensity = args.exaggeration.unwrap_or(0.5);
		ensure!(
			(0.0..=1.0).contains(&intensity),
			"exaggeration must be between 0 and 1, as in MapLibre, not {intensity}"
		);

		let style = Style {
			azimuth: args.azimuth.unwrap_or(335.0).to_radians() + PI,
			intensity,
			shadow: Style::premultiplied(args.shadow_color.as_ref(), [0, 0, 0, 255]),
			highlight: Style::premultiplied(args.highlight_color.as_ref(), [255, 255, 255, 255]),
			accent: Style::premultiplied(args.accent_color.as_ref(), [0, 0, 0, 255]),
		};

		let mut metadata = source.metadata().clone();
		// The shading is translucent, which a JPEG cannot hold.
		if *metadata.tile_format() == TileFormat::JPG {
			metadata.set_tile_format(TileFormat::PNG);
		}
		let mut tilejson = source.tilejson().clone();
		tilejson.tile_schema = Some(TileSchema::RasterRGBA);
		metadata.update_tilejson(&mut tilejson);

		Ok(Operation {
			inner: Arc::new(Inner {
				source,
				metadata,
				tilejson,
				style,
				encoding,
				heights: Cache::builder()
					.max_capacity(CACHE_BYTES)
					.weigher(|_, heights: &Option<Arc<Heights>>| {
						heights
							.as_ref()
							.map_or(256, |h| u32::try_from(h.meters.len() * 4).unwrap_or(u32::MAX))
					})
					.build(),
				shared: SharedWork::new(),
				runtime: factory.runtime(),
			}),
		})
	}

	/// The decoded heights of the source tile at `coord`, read once for all
	/// the tiles that need them.
	async fn heights(&self, coord: TileCoord) -> Result<Option<Arc<Heights>>> {
		let inner = &self.inner;
		inner
			.shared
			.run(coord, || {
				inner.heights.try_get_with(coord, async move {
					let Some(tile) = inner.source.tile(&coord).await? else {
						return Ok(None);
					};
					let encoding = inner.encoding;
					let heights = tokio::task::spawn_blocking(move || Heights::decode(coord, tile, encoding))
						.await
						.map_err(|e| anyhow!("decoding task failed: {e}"))??;
					Ok(Some(Arc::new(heights)))
				})
			})
			.await
	}

	/// The hillshade tile at `coord`, from its heights and its neighbours'.
	async fn shaded(&self, coord: TileCoord) -> Result<Option<Tile>> {
		let Some(center) = self.heights(coord).await? else {
			return Ok(None);
		};
		// At zoom 0 the tile is its own neighbour across the antimeridian,
		// which the cache answers like any other.
		let coords: Vec<Option<TileCoord>> = neighbour_coords(&coord).into_iter().flatten().collect();
		let found = try_join_all(coords.iter().map(|c| async move {
			match c {
				Some(c) => self.heights(*c).await,
				None => Ok(None),
			}
		}))
		.await?;
		let mut neighbours: [[Option<Arc<Heights>>; 3]; 3] = Default::default();
		for (i, heights) in found.into_iter().enumerate() {
			neighbours[i / 3][i % 3] = heights;
		}

		let style = self.inner.style;
		let format = *self.inner.metadata.tile_format();
		let image = tokio::task::spawn_blocking(move || {
			let grid = center.with_border(&neighbours);
			shade(&grid, center.size, &coord, &style)
		})
		.await
		.map_err(|e| anyhow!("shading task failed: {e}"))?;
		Ok(Some(Tile::from_image(DynamicImage::ImageRgba8(image), format)?))
	}

	/// Reads `chunk` and the ring of tiles around it with one request to the
	/// source, and caches the heights of every tile not cached yet, so that
	/// shading the chunk asks the source for nothing more.
	async fn prefetch(&self, chunk: TileBBox) -> Result<()> {
		let max = (1u32 << chunk.level()) - 1;
		let ring = TileBBox::from_min_and_max(
			chunk.level(),
			chunk.x_min()?.saturating_sub(1),
			chunk.y_min()?.saturating_sub(1),
			(chunk.x_max()? + 1).min(max),
			(chunk.y_max()? + 1).min(max),
		)?;
		let encoding = self.inner.encoding;
		let mut decoded = self
			.inner
			.source
			.tile_stream(ring)
			.await?
			.map_parallel_try(move |coord, tile| Heights::decode(coord, tile, encoding));
		let mut arrived = std::collections::HashSet::new();
		while let Some((coord, heights)) = decoded.next().await {
			arrived.insert(coord);
			if !self.inner.heights.contains_key(&coord) {
				self.inner.heights.insert(coord, Some(Arc::new(heights?))).await;
			}
		}
		let ring_coords: Vec<TileCoord> = ring.iter_coords().collect();
		for coord in ring_coords {
			if !arrived.contains(&coord) && !self.inner.heights.contains_key(&coord) {
				self.inner.heights.insert(coord, None).await;
			}
		}
		Ok(())
	}

	/// The hillshade tiles of `chunk`.
	async fn chunk_tiles(&self, chunk: TileBBox) -> Vec<(TileCoord, Tile)> {
		let runtime = &self.inner.runtime;
		if let Err(error) = self.prefetch(chunk).await {
			runtime.record_error(&format!("dem_hillshade reading {chunk:?}"), &error);
			return Vec::new();
		}
		stream::iter(chunk.iter_coords().collect::<Vec<_>>())
			.map(|coord| async move { (coord, self.shaded(coord).await) })
			.buffer_unordered(ConcurrencyLimits::default().cpu_bound)
			.filter_map(|(coord, result)| async move {
				match result {
					Ok(tile) => tile.map(|tile| (coord, tile)),
					Err(error) => {
						runtime.record_error(&format!("dem_hillshade tile {}", coord.to_json()), &error);
						None
					}
				}
			})
			.collect()
			.await
	}

	/// Tiles one side of a chunk spans, smaller for larger tiles so that a
	/// chunk takes about the same memory.
	fn chunk_tiles_per_side(&self) -> u32 {
		let size = self.inner.tilejson.tile_size.map_or(256, |s| u32::from(s.size()));
		(CHUNK_TILES_AT_256 * 256 / size.max(1)).clamp(4, CHUNK_TILES_AT_256)
	}
}

#[async_trait]
impl TileSource for Operation {
	fn metadata(&self) -> &TileSourceMetadata {
		&self.inner.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.inner.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("dem_hillshade", self.inner.source.source_type())
	}

	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		self.shaded(*coord).await
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let chunks: Vec<TileBBox> = bbox.iter_grid(self.chunk_tiles_per_side()).collect();
		let this = self.clone();
		let tiles = stream::iter(chunks)
			.map(move |chunk| {
				let this = this.clone();
				async move { this.chunk_tiles(chunk).await }
			})
			.buffer_unordered(CHUNKS_IN_FLIGHT)
			.flat_map(stream::iter);
		Ok(TileStream::from_stream(Box::pin(tiles)))
	}

	/// The source's: every tile it has is shaded, flat ground included.
	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.inner.source.tile_coord_stream(bbox).await
	}
}

crate::operations::macros::define_transform_factory!("dem_hillshade", Args, Operation, requires: Raster);

#[cfg(test)]
mod tests {
	use imageproc::image::RgbImage;
	use versatiles_core::TilePyramid;

	use super::*;
	use crate::{factory::OperationFactoryTrait, helpers::dummy_image_source::DummyImageSource};

	/// Zoom 15 at the equator: no exaggeration, and a latitude correction
	/// within a hair of 1.
	const LEVEL: u8 = 15;
	const ORIGIN: u32 = 1 << 14;

	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		reason = "test values fit 24 bits, and each byte is masked out of them"
	)]
	fn encode(encoding: DemEncoding, meters: f64) -> [u8; 3] {
		let raw = match encoding {
			DemEncoding::Mapbox => ((meters + 10000.0) * 10.0).round(),
			DemEncoding::Terrarium => ((meters + 32768.0) * 256.0).round(),
		} as u32;
		[(raw >> 16) as u8, (raw >> 8) as u8, raw as u8]
	}

	/// A source whose tiles together are one surface, `meters(x, y)` at the
	/// global pixel `(x, y)` counted from the northwest corner of tile
	/// `ORIGIN, ORIGIN`, over the 3 × 3 tiles around it.
	fn surface(encoding: DemEncoding, meters: fn(f64, f64) -> f64) -> Result<DummyImageSource> {
		let bbox = TileBBox::from_min_and_max(LEVEL, ORIGIN - 1, ORIGIN - 1, ORIGIN + 1, ORIGIN + 1)?;
		let pyramid = TilePyramid::from_tile_coords(bbox.iter_coords());
		let mut source = DummyImageSource::new(
			move |coord| {
				let mut image = RgbImage::new(256, 256);
				let (ox, oy) = (
					(f64::from(coord.x) - f64::from(ORIGIN)) * 256.0,
					(f64::from(coord.y) - f64::from(ORIGIN)) * 256.0,
				);
				for (x, y, pixel) in image.enumerate_pixels_mut() {
					pixel.0 = encode(encoding, meters(ox + f64::from(x), oy + f64::from(y)));
				}
				Tile::from_image(DynamicImage::ImageRgb8(image), TileFormat::PNG).ok()
			},
			TileFormat::PNG,
			Some(pyramid),
		)?;
		source.tilejson_mut().tile_schema = Some(super::super::encoding::to_tile_schema(encoding));
		Ok(source)
	}

	async fn hillshade(source: Result<DummyImageSource>, vpl: &str) -> Result<Operation> {
		Operation::build(
			VPLNode::try_from_str(vpl)?,
			Box::new(source?),
			&PipelineFactory::new_dummy(),
		)
		.await
	}

	async fn shaded_image(op: &Operation, x: u32, y: u32) -> Result<RgbaImage> {
		let tile = op.tile(&TileCoord::new(LEVEL, x, y)?).await?.expect("a tile");
		Ok(tile.into_image()?.to_rgba8())
	}

	#[test]
	fn factory_docs_name_the_parameters() {
		let docs = Factory {}.docs();
		for name in [
			"azimuth",
			"exaggeration",
			"shadow_color",
			"highlight_color",
			"accent_color",
			"encoding",
		] {
			assert!(docs.contains(name), "{name} missing from {docs}");
		}
	}

	#[tokio::test]
	async fn flat_terrain_is_fully_transparent() -> Result<()> {
		let op = hillshade(surface(DemEncoding::Mapbox, |_, _| 123.0), "dem_hillshade").await?;
		let image = shaded_image(&op, ORIGIN, ORIGIN).await?;
		assert!(image.pixels().all(|p| p.0[3] == 0));
		Ok(())
	}

	/// A plane rising 1 m east per metre of ground: a derivative of exactly 1
	/// in MapLibre's terms. The expected colour is MapLibre's `standard`
	/// shading worked out by hand for that derivative.
	#[tokio::test]
	async fn a_known_slope_gives_the_expected_shade() -> Result<()> {
		let pixel = WORLD_WIDTH_METERS / f64::from(1u32 << LEVEL) / 256.0;
		let op = hillshade(surface(DemEncoding::Terrarium, |x, _| x * 4.776), "dem_hillshade").await?;
		let shaded = shaded_image(&op, ORIGIN, ORIGIN).await?;
		let p = shaded.get_pixel(128, 128).0;

		// d = 1 (4.776 m per pixel against a pixel of 4.777 m), so
		// slope = atan(0.625); aspect = π for a slope facing west.
		assert!((4.776 / pixel - 1.0).abs() < 1e-3);
		let slope = 0.625f64.atan();
		let facing = (((PI + 335f64.to_radians() + PI) / PI + 0.5).rem_euclid(2.0) - 1.0).abs();
		let shade_alpha = slope.sin();
		let alpha = shade_alpha + (1.0 - slope.cos()) * (1.0 - shade_alpha);
		let grey = facing * shade_alpha / alpha;
		#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "0..=255")]
		let expected = [grey, grey, grey, alpha].map(|v| (v * 255.0).round() as u8);
		for k in 0..4 {
			assert!(p[k].abs_diff(expected[k]) <= 1, "pixel {p:?}, expected {expected:?}");
		}
		Ok(())
	}

	/// A slope facing the light is lit, one facing away is shaded.
	#[tokio::test]
	async fn slopes_facing_the_light_are_lit() -> Result<()> {
		// Light from the west: a slope rising to the east faces it.
		let facing = hillshade(
			surface(DemEncoding::Mapbox, |x, _| x * 5.0),
			"dem_hillshade azimuth=270",
		)
		.await?;
		let away = hillshade(
			surface(DemEncoding::Mapbox, |x, _| -x * 5.0),
			"dem_hillshade azimuth=270",
		)
		.await?;
		let lit = shaded_image(&facing, ORIGIN, ORIGIN).await?.get_pixel(100, 100).0;
		let dark = shaded_image(&away, ORIGIN, ORIGIN).await?.get_pixel(100, 100).0;
		assert!(lit[0] > 200, "{lit:?}");
		assert!(dark[0] < 50, "{dark:?}");
		Ok(())
	}

	/// A colour's alpha weakens its part of the shading, as in MapLibre, where
	/// colours are premultiplied.
	#[tokio::test]
	async fn a_translucent_colour_shades_more_lightly() -> Result<()> {
		let opaque = hillshade(
			surface(DemEncoding::Mapbox, |x, _| x * 5.0),
			"dem_hillshade azimuth=270",
		)
		.await?;
		let faint = hillshade(
			surface(DemEncoding::Mapbox, |x, _| x * 5.0),
			"dem_hillshade azimuth=270 highlight_color=FFFFFF40",
		)
		.await?;
		let strong = shaded_image(&opaque, ORIGIN, ORIGIN).await?.get_pixel(100, 100).0;
		let weak = shaded_image(&faint, ORIGIN, ORIGIN).await?.get_pixel(100, 100).0;
		assert!(weak[3] < strong[3], "{weak:?} against {strong:?}");
		Ok(())
	}

	/// The slope is constant on a plane, so the shade is too, row by row (the
	/// latitude correction differs between rows). Pixels at a tile's edge see
	/// the same slope only if their neighbours come from the adjacent tiles.
	#[tokio::test]
	async fn there_are_no_seams_across_tile_borders() -> Result<()> {
		let op = hillshade(surface(DemEncoding::Mapbox, |x, y| x * 3.0 + y * 2.0), "dem_hillshade").await?;
		let west = shaded_image(&op, ORIGIN - 1, ORIGIN).await?;
		let center = shaded_image(&op, ORIGIN, ORIGIN).await?;
		for y in 0..256 {
			let expected = center.get_pixel(128, y).0;
			for x in [0, 1, 254, 255] {
				assert_eq!(center.get_pixel(x, y).0, expected, "pixel {x}, {y}");
			}
			assert_eq!(west.get_pixel(255, y).0, expected, "west edge, row {y}");
		}
		let north = shaded_image(&op, ORIGIN, ORIGIN - 1).await?;
		let south = shaded_image(&op, ORIGIN, ORIGIN + 1).await?;
		// Neighbouring rows across the border differ by latitude only.
		for (a, b) in [
			(north.get_pixel(128, 255).0, center.get_pixel(128, 0).0),
			(center.get_pixel(128, 255).0, south.get_pixel(128, 0).0),
		] {
			for k in 0..4 {
				assert!(a[k].abs_diff(b[k]) <= 1, "{a:?} against {b:?}");
			}
		}
		Ok(())
	}

	/// Where there is no neighbour, the edge is repeated rather than read as
	/// a cliff down to nothing.
	#[tokio::test]
	async fn the_edge_of_the_data_repeats_its_edge_pixels() -> Result<()> {
		let op = hillshade(surface(DemEncoding::Mapbox, |_, _| 500.0), "dem_hillshade").await?;
		// The northwest corner of the data: no tile above or left of it.
		let corner = shaded_image(&op, ORIGIN - 1, ORIGIN - 1).await?;
		assert!(
			corner.pixels().all(|p| p.0[3] == 0),
			"flat ground has no slope at the edge either"
		);
		Ok(())
	}

	#[tokio::test]
	async fn both_encodings_shade_alike() -> Result<()> {
		let mapbox = hillshade(surface(DemEncoding::Mapbox, |x, y| x * 2.0 - y), "dem_hillshade").await?;
		let terrarium = hillshade(surface(DemEncoding::Terrarium, |x, y| x * 2.0 - y), "dem_hillshade").await?;
		let (a, b) = (
			shaded_image(&mapbox, ORIGIN, ORIGIN).await?,
			shaded_image(&terrarium, ORIGIN, ORIGIN).await?,
		);
		for (pa, pb) in a.pixels().zip(b.pixels()) {
			for k in 0..4 {
				assert!(pa.0[k].abs_diff(pb.0[k]) <= 1, "{pa:?} against {pb:?}");
			}
		}
		Ok(())
	}

	#[tokio::test]
	async fn the_stream_gives_the_tiles_one_at_a_time_would() -> Result<()> {
		let op = hillshade(
			surface(DemEncoding::Mapbox, |x, y| (x * 0.05).sin() * 300.0 + y),
			"dem_hillshade",
		)
		.await?;
		let bbox = TileBBox::from_min_and_max(LEVEL, ORIGIN - 1, ORIGIN - 1, ORIGIN + 1, ORIGIN + 1)?;
		let streamed = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(streamed.len(), 9);

		let fresh = hillshade(
			surface(DemEncoding::Mapbox, |x, y| (x * 0.05).sin() * 300.0 + y),
			"dem_hillshade",
		)
		.await?;
		for (coord, tile) in streamed {
			let single = fresh.tile(&coord).await?.expect("a tile").into_image()?.to_rgba8();
			assert_eq!(tile.into_image()?.to_rgba8(), single, "tile {coord:?}");
		}
		Ok(())
	}

	#[test]
	fn neighbours_wrap_around_the_antimeridian_and_stop_at_the_poles() -> Result<()> {
		let west_edge = neighbour_coords(&TileCoord::new(2, 0, 0)?);
		assert_eq!(west_edge[1][0], Some(TileCoord::new(2, 3, 0)?));
		assert_eq!(west_edge[0], [None, None, None]);
		assert_eq!(west_edge[2][2], Some(TileCoord::new(2, 1, 1)?));

		let world = neighbour_coords(&TileCoord::new(0, 0, 0)?);
		assert_eq!(world[1], [Some(TileCoord::new(0, 0, 0)?); 3]);
		Ok(())
	}

	#[tokio::test]
	async fn the_result_is_rgba_and_a_jpeg_source_becomes_png() -> Result<()> {
		let op = hillshade(surface(DemEncoding::Mapbox, |_, _| 0.0), "dem_hillshade").await?;
		assert_eq!(op.tilejson().tile_schema, Some(TileSchema::RasterRGBA));

		let source = DummyImageSource::from_color(&[1, 134, 160], 256, TileFormat::JPG, None)?;
		let op = Operation::build(
			VPLNode::try_from_str("dem_hillshade encoding=mapbox")?,
			Box::new(source),
			&PipelineFactory::new_dummy(),
		)
		.await?;
		assert_eq!(*op.metadata().tile_format(), TileFormat::PNG);
		Ok(())
	}

	#[tokio::test]
	async fn an_exaggeration_outside_0_to_1_is_refused() {
		for value in ["-0.1", "1.5"] {
			let error = hillshade(
				surface(DemEncoding::Mapbox, |_, _| 0.0),
				&format!("dem_hillshade exaggeration={value}"),
			)
			.await
			.unwrap_err();
			assert!(format!("{error:#}").contains("between 0 and 1"), "{error:#}");
		}
	}

	#[tokio::test]
	async fn a_source_that_is_not_mapbox_or_terrarium_is_refused() {
		let factory = PipelineFactory::new_dummy();
		for vpl in [
			"from_debug format=png | dem_hillshade",
			"from_debug format=png | meta_update schema=\"dem/versatiles\" | dem_hillshade",
		] {
			let error = factory.operation_from_vpl(vpl).await.unwrap_err();
			assert!(format!("{error:#}").contains("encoding"), "{vpl}: {error:#}");
		}
	}
}
