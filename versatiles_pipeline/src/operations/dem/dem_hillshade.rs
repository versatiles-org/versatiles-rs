use std::{
	collections::{HashMap, HashSet},
	f64::consts::PI,
	sync::{
		Arc, Mutex,
		atomic::{AtomicU64, Ordering},
	},
};

use anyhow::{Result, anyhow, bail, ensure};
use async_trait::async_trait;
use futures::{StreamExt, future::try_join_all, stream};
use imageproc::image::{DynamicImage, Rgba, RgbaImage};
use moka::{future::Cache, policy::EvictionPolicy};
use tokio::sync::{Semaphore, watch};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, TilesRuntime};
use versatiles_core::{
	CancelToken, ConcurrencyLimits, SharedWork, TileBBox, TileCoord, TileFormat, TileJSON, TileSchema, TileSize,
	TileStream, WORLD_SIZE, bounded_number,
};
use versatiles_derive::context;
use versatiles_image::color::HexColor;

use super::{
	color_ramp::ColorRamp,
	encoding::{DemEncoding, decode_elevation, resolve_encoding},
	slope::maplibre_slope_exaggeration,
};
use crate::{PipelineFactory, vpl::VPLNode};

bounded_number! {
	/// The intensity of the shading, over the range MapLibre accepts.
	Exaggeration: f64, 0.0..=1.0;
}

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Renders DEM tiles as hillshading, the way MapLibre's `hillshade` layer shades them.
///
/// The result is RGBA tiles that are transparent on flat ground and shade
/// slopes by their steepness and the direction they face. They are an ordinary
/// raster source for any map client. Unlike elevation tiles, they can be
/// encoded lossily: with a following `raster_format format=webp quality=70`, a
/// mountain tile at zoom 8 comes out about a quarter smaller than the elevation
/// tile it is computed from, and with `format=avif quality=50` about 60 %
/// smaller. Flat land is the exception: its faint alpha comes out smaller
/// lossless, so for a large area add `lossless_if_smaller=true` to the WebP
/// one, which picks lossy or lossless tile by tile.
///
/// The shading is MapLibre's default `standard` method, including the slope
/// exaggeration it applies below zoom 15 and its correction for latitude, so a
/// tile looks as a live `hillshade` layer would at the same zoom. The colours
/// are given as `RRGGBB`, `RRGGBBAA` or `[r,g,b,a]`, and an alpha below `FF`
/// makes that part of the shading lighter.
///
/// With a `ramp`, each pixel is the colour for its elevation with the shading
/// drawn over it, which gives a coloured relief in one step. The ramp is written
/// as in `dem_colorize`, and `ramp=default` is its hypsometric tint. A ramp with
/// only opaque stops gives RGB tiles, and a JPEG source stays JPEG; one with a
/// translucent stop gives RGBA tiles, and a JPEG source becomes PNG.
///
/// The slopes at a tile's edge are computed with the pixels of the adjacent
/// tiles, so there are no seams. The tiles are read in chunks, several ahead
/// of the ones being shaded, and the edges of every tile are kept for the
/// chunks next to it. Across the antimeridian the neighbour wraps around.
/// Where there is none, at the poles or the edge of the data, the edge pixels
/// are repeated.
struct Args {
	/// Direction the light comes from, in degrees clockwise from north. Defaults to `335`.
	#[vpl(default = "335")]
	azimuth: Option<f64>,
	/// Intensity of the shading, from 0 to 1. Defaults to `0.5`.
	#[vpl(default = "0.5")]
	exaggeration: Option<Exaggeration>,
	/// Colour of slopes facing away from the light. Defaults to `000000`.
	#[vpl(default = "000000")]
	shadow_color: Option<HexColor>,
	/// Colour of slopes facing the light. Defaults to `FFFFFF`.
	#[vpl(default = "FFFFFF")]
	highlight_color: Option<HexColor>,
	/// Colour that accentuates steep terrain, such as cliffs and gorges. Defaults to `000000`.
	#[vpl(default = "000000")]
	accent_color: Option<HexColor>,
	/// Colours under the shading, as `elevation:colour,…` or `default`. Defaults to none.
	ramp: Option<ColorRamp>,
	/// DEM encoding of the source. Defaults to the encoding its tile schema implies.
	encoding: Option<DemEncoding>,
}

/// Decoded heights the cache may hold, in bytes: the chunks held, with room
/// to spare.
const CACHE_BYTES: u64 = 512 * 1024 * 1024;

/// Edges of decoded tiles the cache may hold, in bytes: 8 KB a tile at 512 px.
const EDGES_BYTES: u64 = 64 * 1024 * 1024;

/// Tiles one side of a chunk spans at 256 px: 16 × 16 decoded tiles are
/// 64 MiB. Larger tiles get smaller chunks of the same size in bytes.
const CHUNK_TILES_AT_256: u32 = 16;

/// Chunks shaded at once in a stream.
const CHUNKS_SHADING: usize = 2;

/// Chunks read ahead of the ones being shaded, so that reading and shading
/// overlap. Without it, shading waited for every chunk to arrive, and the
/// network for every chunk to be shaded (#310).
const CHUNKS_AHEAD: usize = 4;

/// Chunks whose heights are held at once, across all streams of the
/// operation: a consumer that opens many streams at once, such as a writer
/// buffering small blocks, would otherwise read more than the cache holds.
const CHUNKS_HELD: usize = CHUNKS_SHADING + CHUNKS_AHEAD;

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
	fn with_border(&self, neighbours: &[[Option<Arc<Edges>>; 3]; 3]) -> Vec<f32> {
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

/// The outermost heights of a tile: all that shading its neighbours takes from
/// it, and a 128th of its heights at 512 px.
#[derive(Debug)]
struct Edges {
	size: u32,
	top: Vec<f32>,
	bottom: Vec<f32>,
	left: Vec<f32>,
	right: Vec<f32>,
}

impl Edges {
	fn of(heights: &Heights) -> Edges {
		let size = heights.size;
		let last = size.saturating_sub(1);
		Edges {
			size,
			top: (0..size).map(|x| heights.at(x, 0)).collect(),
			bottom: (0..size).map(|x| heights.at(x, last)).collect(),
			left: (0..size).map(|y| heights.at(0, y)).collect(),
			right: (0..size).map(|y| heights.at(last, y)).collect(),
		}
	}

	/// The height at `(x, y)`, which lies on the edge.
	fn at(&self, x: u32, y: u32) -> f32 {
		let last = self.size - 1;
		if y == 0 {
			self.top[x as usize]
		} else if y == last {
			self.bottom[x as usize]
		} else if x == 0 {
			self.left[y as usize]
		} else {
			debug_assert_eq!(x, last, "({x}, {y}) is not on the edge");
			self.right[y as usize]
		}
	}

	fn weight(&self) -> u32 {
		16 * self.size + 64
	}
}

/// Shades a tile of `size` px from its heights with a border, `(size + 2)²`
/// values row by row, as MapLibre's `standard` method shades it at zoom
/// `coord.level`.
///
/// Follows `hillshade_prepare.fragment.glsl` for the slope, including the
/// clamp that storing it in 8 bits imposes there, and `standard_hillshade` in
/// `hillshade.fragment.glsl` for the colour. With a `ramp`, the shading is drawn
/// over the ramp's colour, exactly as if it were a layer on top of that colour,
/// but before either is rounded to 8 bits.
#[expect(
	clippy::many_single_char_names,
	reason = "`a` to `i` are the 3×3 window as MapLibre's shader names it, which makes the two easy to compare"
)]
fn shade(grid: &[f32], size: u32, coord: &TileCoord, style: &Style, ramp: Option<&ColorRamp>) -> RgbaImage {
	let side = size as usize + 2;
	let at = |x: usize, y: usize| f64::from(grid[y * side + x]);

	let tiles = f64::from(1u32 << coord.level);
	let pixel_meters = WORLD_SIZE / tiles / f64::from(size);
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
			*pixel = unpremultiply(match ramp {
				Some(ramp) => {
					let under = ramp.premultiplied_at(at(x, y));
					[0, 1, 2, 3].map(|k| color[k] + under[k] * (1.0 - color[3]))
				}
				None => color,
			});
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
	/// The colours under the shading, if any.
	ramp: Option<ColorRamp>,
	encoding: DemEncoding,
	/// The size of the source's tiles, declared or measured.
	tile_size: TileSize,
	/// Decoded source tiles; `None` is a tile the source lacks.
	heights: Cache<TileCoord, Option<Arc<Heights>>>,
	/// The edges of decoded source tiles, kept long after their heights, so
	/// that a chunk finds the edges of the chunks before it rather than reading
	/// them again; `None` is a tile the source lacks.
	edges: Cache<TileCoord, Option<Arc<Edges>>>,
	/// Which requests wait for which entry of `heights` (#265).
	shared: SharedWork<TileCoord>,
	/// Permits for the chunks whose heights are held, `CHUNKS_HELD` of them.
	chunks: Arc<Semaphore>,
	/// The chunks being read, by the cell of the chunk grid they lie in: a
	/// tile in one of them is waited for rather than read a second time.
	reading: Mutex<HashMap<(u8, u32, u32), Vec<ChunkRead>>>,
	/// Numbers the entries of `reading`.
	reads_started: AtomicU64,
	runtime: TilesRuntime,
}

/// A chunk being read, which those waiting for it watch.
struct ChunkRead {
	id: u64,
	bbox: TileBBox,
	done: watch::Receiver<()>,
}

/// Registers a chunk as being read, until it is dropped.
struct Reading {
	operation: Operation,
	cell: (u8, u32, u32),
	id: u64,
	_done: watch::Sender<()>,
}

impl Drop for Reading {
	fn drop(&mut self) {
		let mut reading = self.operation.inner.reading.lock().expect("poisoned mutex");
		if let Some(reads) = reading.get_mut(&self.cell) {
			reads.retain(|read| read.id != self.id);
			if reads.is_empty() {
				reading.remove(&self.cell);
			}
		}
	}
}

impl std::fmt::Debug for Operation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("dem_hillshade")
			.field("style", &self.inner.style)
			.field("ramp", &self.inner.ramp)
			.field("encoding", &self.inner.encoding)
			.finish_non_exhaustive()
	}
}

impl Operation {
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, factory: &PipelineFactory) -> Result<Operation> {
		Self::build_with_cache(vpl_node, source, factory, CACHE_BYTES).await
	}

	/// Builds the operation with a cache of `cache_bytes`, so that a test can
	/// fill the cache without decoding half a gigabyte.
	#[context("Building dem_hillshade operation in VPL node {:?}", vpl_node.name)]
	async fn build_with_cache(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &PipelineFactory,
		cache_bytes: u64,
	) -> Result<Operation> {
		let args = Args::from_vpl_node(&vpl_node)?;
		let encoding = resolve_encoding(args.encoding, source.tilejson().tile_schema.as_ref())?;
		// The size of a chunk follows from the size of a tile, which a source
		// need not declare. Mapterhorn's planet does not, and its tiles of
		// 512 px were read in chunks meant for 256 px, four times as many bytes
		// as the cache was sized for.
		let tile_size = match source.tilejson().tile_size {
			Some(size) => size,
			None => source.measure_tile_size().await?.unwrap_or(TileSize::Size256),
		};

		let style = Style {
			azimuth: args.azimuth.unwrap_or(335.0).to_radians() + PI,
			intensity: args.exaggeration.map_or(0.5, Exaggeration::get),
			shadow: Style::premultiplied(args.shadow_color.as_ref(), [0, 0, 0, 255]),
			highlight: Style::premultiplied(args.highlight_color.as_ref(), [255, 255, 255, 255]),
			accent: Style::premultiplied(args.accent_color.as_ref(), [0, 0, 0, 255]),
		};

		// The shading alone is translucent, and so is what it covers unless a
		// ramp makes every pixel opaque. A JPEG holds only the opaque.
		let opaque = args.ramp.as_ref().is_some_and(ColorRamp::is_opaque);
		let mut metadata = source.metadata().clone();
		if *metadata.tile_format() == TileFormat::JPG && !opaque {
			metadata.set_tile_format(TileFormat::PNG);
		}
		let mut tilejson = source.tilejson().clone();
		tilejson.tile_schema = Some(if opaque {
			TileSchema::RasterRGB
		} else {
			TileSchema::RasterRGBA
		});
		metadata.update_tilejson(&mut tilejson);

		// Both caches are LRU, not moka's default TinyLFU: a tile just read has
		// been used less often than the tiles of finished chunks, so TinyLFU
		// would turn it away once the cache is full, and its chunk would read it
		// again.
		Ok(Operation {
			inner: Arc::new(Inner {
				source,
				metadata,
				tilejson,
				style,
				ramp: args.ramp,
				encoding,
				tile_size,
				heights: Cache::builder()
					.max_capacity(cache_bytes)
					.eviction_policy(EvictionPolicy::lru())
					.weigher(|_, heights: &Option<Arc<Heights>>| {
						heights
							.as_ref()
							.map_or(256, |h| u32::try_from(h.meters.len() * 4).unwrap_or(u32::MAX))
					})
					.build(),
				edges: Cache::builder()
					.max_capacity(EDGES_BYTES)
					.eviction_policy(EvictionPolicy::lru())
					.weigher(|_, edges: &Option<Arc<Edges>>| edges.as_ref().map_or(256, |e| e.weight()))
					.build(),
				shared: SharedWork::new(),
				chunks: Arc::new(Semaphore::new(CHUNKS_HELD)),
				reading: Mutex::default(),
				reads_started: AtomicU64::new(0),
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
					let heights = versatiles_core::stage::spawn_blocking(move || Heights::decode(coord, tile, encoding))
						.await
						.map_err(|e| anyhow!("decoding task failed: {e}"))??;
					Ok(Some(Arc::new(heights)))
				})
			})
			.await
	}

	/// The edges of the source tile at `coord`.
	async fn edges(&self, coord: TileCoord) -> Result<Option<Arc<Edges>>> {
		if let Some(edges) = self.inner.edges.get(&coord).await {
			return Ok(edges);
		}
		let edges = self.heights(coord).await?.map(|heights| Arc::new(Edges::of(&heights)));
		self.inner.edges.insert(coord, edges.clone()).await;
		Ok(edges)
	}

	/// The hillshade tile at `coord`, from its heights and its neighbours'
	/// edges.
	async fn shaded(&self, coord: TileCoord) -> Result<Option<Tile>> {
		let Some(center) = self.heights(coord).await? else {
			return Ok(None);
		};
		// At zoom 0 the tile is its own neighbour across the antimeridian,
		// which the cache answers like any other.
		let coords: Vec<Option<TileCoord>> = neighbour_coords(&coord).into_iter().flatten().collect();
		let found = try_join_all(coords.iter().map(|c| async move {
			match c {
				Some(c) => self.edges(*c).await,
				None => Ok(None),
			}
		}))
		.await?;
		let mut neighbours: [[Option<Arc<Edges>>; 3]; 3] = Default::default();
		for (i, edges) in found.into_iter().enumerate() {
			neighbours[i / 3][i % 3] = edges;
		}

		let inner = Arc::clone(&self.inner);
		let opaque = inner.ramp.as_ref().is_some_and(ColorRamp::is_opaque);
		let format = *inner.metadata.tile_format();
		let image = versatiles_core::stage::spawn_blocking(move || {
			let grid = center.with_border(&neighbours);
			shade(&grid, center.size, &coord, &inner.style, inner.ramp.as_ref())
		})
		.await
		.map_err(|e| anyhow!("shading task failed: {e}"))?;
		let image = DynamicImage::ImageRgba8(image);
		let image = if opaque {
			DynamicImage::ImageRgb8(image.into_rgb8())
		} else {
			image
		};
		Ok(Some(Tile::from_image(image, format)?))
	}

	/// The cell of the chunk grid `coord` lies in.
	fn cell(&self, coord: &TileCoord) -> (u8, u32, u32) {
		let side = self.chunk_tiles_per_side();
		(coord.level, coord.x / side, coord.y / side)
	}

	/// Registers `chunk` as being read, until the guard is dropped.
	fn start_reading(&self, chunk: TileBBox) -> Option<Reading> {
		let first = chunk.iter_coords().next()?;
		let cell = self.cell(&first);
		let id = self.inner.reads_started.fetch_add(1, Ordering::Relaxed);
		let (done, watched) = watch::channel(());
		let read = ChunkRead {
			id,
			bbox: chunk,
			done: watched,
		};
		self
			.inner
			.reading
			.lock()
			.expect("poisoned mutex")
			.entry(cell)
			.or_default()
			.push(read);
		Some(Reading {
			operation: self.clone(),
			cell,
			id,
			_done: done,
		})
	}

	/// What the chunks being read hold of `coords`, to wait for.
	fn reads_of(&self, coords: &[TileCoord]) -> Vec<watch::Receiver<()>> {
		let reading = self.inner.reading.lock().expect("poisoned mutex");
		let mut ids = HashSet::new();
		let mut found = Vec::new();
		for coord in coords {
			for read in reading.get(&self.cell(coord)).into_iter().flatten() {
				if read.bbox.includes_coord(coord) && ids.insert(read.id) {
					found.push(read.done.clone());
				}
			}
		}
		found
	}

	/// The tiles around `chunk` whose edges shading it takes.
	fn ring(chunk: &TileBBox) -> Vec<TileCoord> {
		let mut ring = HashSet::new();
		for coord in chunk.iter_coords() {
			for neighbour in neighbour_coords(&coord).into_iter().flatten().flatten() {
				if !chunk.includes_coord(&neighbour) {
					ring.insert(neighbour);
				}
			}
		}
		ring.into_iter().collect()
	}

	/// Reads `chunk` with one request to the source and caches its heights and
	/// edges, then reads the edges of its ring, except those of the chunk read
	/// next, `next`, and of the chunks being read: shading the chunk waits for
	/// those. The rest, typically a row along the bottom, is one request more.
	///
	/// `reading` registers the chunk as being read until this returns; it is
	/// taken before the task that runs this is spawned, so that the chunk
	/// before this one cannot miss it.
	async fn prefetch(&self, chunk: TileBBox, next: Option<TileBBox>, reading: Option<Reading>) -> Result<()> {
		let inner = &self.inner;
		let _reading = reading;

		if chunk.iter_coords().any(|coord| !inner.heights.contains_key(&coord)) {
			let encoding = inner.encoding;
			let mut decoded = inner
				.source
				.tile_stream(chunk)
				.await?
				.map_parallel_try(move |coord, tile| Heights::decode(coord, tile, encoding));
			let mut arrived = HashSet::new();
			while let Some((coord, heights)) = decoded.next().await {
				arrived.insert(coord);
				if !inner.heights.contains_key(&coord) {
					let heights = Arc::new(heights?);
					inner.edges.insert(coord, Some(Arc::new(Edges::of(&heights)))).await;
					inner.heights.insert(coord, Some(heights)).await;
				}
			}
			let absent: Vec<TileCoord> = chunk.iter_coords().filter(|coord| !arrived.contains(coord)).collect();
			for coord in absent {
				if !inner.heights.contains_key(&coord) {
					inner.heights.insert(coord, None).await;
					inner.edges.insert(coord, None).await;
				}
			}
		}

		let ring: Vec<TileCoord> = Self::ring(&chunk)
			.into_iter()
			.filter(|coord| !next.is_some_and(|next| next.includes_coord(coord)))
			.collect();
		self.read_edges(ring, false).await
	}

	/// Caches the edges of `coords`, reading those not cached yet in as few
	/// requests as their arrangement allows. A tile in a chunk being read is
	/// waited for if `wait` is set, and otherwise left to that chunk.
	async fn read_edges(&self, coords: Vec<TileCoord>, wait: bool) -> Result<()> {
		let inner = &self.inner;
		let mut wanted: Vec<TileCoord> = coords
			.into_iter()
			.filter(|coord| !inner.edges.contains_key(coord))
			.collect();
		let reads = self.reads_of(&wanted);
		if wait {
			for mut read in reads {
				// Errs once the read is over, which is what is waited for.
				let _ = read.changed().await;
			}
		} else if !reads.is_empty() {
			let reading = inner.reading.lock().expect("poisoned mutex");
			wanted.retain(|coord| {
				!reading
					.get(&self.cell(coord))
					.is_some_and(|reads| reads.iter().any(|read| read.bbox.includes_coord(coord)))
			});
		}

		let mut unread = Vec::new();
		for coord in wanted {
			if inner.edges.contains_key(&coord) {
				continue;
			}
			match inner.heights.get(&coord).await {
				Some(heights) => {
					let edges = heights.map(|heights| Arc::new(Edges::of(&heights)));
					inner.edges.insert(coord, edges).await;
				}
				None => unread.push(coord),
			}
		}

		let encoding = inner.encoding;
		try_join_all(strips(&unread)?.into_iter().map(|strip| async move {
			let mut decoded = inner
				.source
				.tile_stream(strip)
				.await?
				.map_parallel_try(move |coord, tile| Heights::decode(coord, tile, encoding).map(|h| Edges::of(&h)));
			let mut arrived = HashSet::new();
			while let Some((coord, edges)) = decoded.next().await {
				arrived.insert(coord);
				inner.edges.insert(coord, Some(Arc::new(edges?))).await;
			}
			let absent: Vec<TileCoord> = strip.iter_coords().filter(|coord| !arrived.contains(coord)).collect();
			for coord in absent {
				inner.edges.insert(coord, None).await;
			}
			Ok::<_, anyhow::Error>(())
		}))
		.await?;
		Ok(())
	}

	/// The hillshade tiles of `chunk`, read already.
	async fn shade_chunk(&self, chunk: TileBBox) -> Vec<(TileCoord, Tile)> {
		let runtime = &self.inner.runtime;
		if let Err(error) = self.read_edges(Self::ring(&chunk), true).await {
			runtime.record_error(&format!("dem_hillshade reading around {chunk:?}"), &error);
			return Vec::new();
		}
		let coords: Vec<TileCoord> = chunk.iter_coords().collect();
		let tiles = stream::iter(coords.clone())
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
			.await;
		// Their neighbours take their edges, from the other cache. Left in this
		// one, they would look used more recently than the chunks read ahead,
		// and LRU would drop those instead.
		for coord in coords {
			self.inner.heights.invalidate(&coord).await;
		}
		tiles
	}

	/// Tiles one side of a chunk spans, smaller for larger tiles so that a
	/// chunk takes about the same memory.
	fn chunk_tiles_per_side(&self) -> u32 {
		let size = u32::from(self.inner.tile_size.size());
		(CHUNK_TILES_AT_256 * 256 / size.max(1)).clamp(4, CHUNK_TILES_AT_256)
	}
}

/// Boxes that cover `coords` and nothing else: runs side by side in a row,
/// then runs one above the other in a column for the rest. The ring of a
/// chunk comes out as up to four strips.
fn strips(coords: &[TileCoord]) -> Result<Vec<TileBBox>> {
	let mut sorted = coords.to_vec();
	sorted.sort_by_key(|coord| (coord.level, coord.y, coord.x));
	sorted.dedup();

	let mut strips = Vec::new();
	let mut single = Vec::new();
	for run in sorted.chunk_by(|a, b| a.level == b.level && a.y == b.y && a.x + 1 == b.x) {
		let (first, last) = (run[0], run[run.len() - 1]);
		if run.len() > 1 {
			strips.push(TileBBox::from_min_and_max(
				first.level,
				first.x,
				first.y,
				last.x,
				last.y,
			)?);
		} else {
			single.push(first);
		}
	}
	single.sort_by_key(|coord| (coord.level, coord.x, coord.y));
	for run in single.chunk_by(|a, b| a.level == b.level && a.x == b.x && a.y + 1 == b.y) {
		let (first, last) = (run[0], run[run.len() - 1]);
		strips.push(TileBBox::from_min_and_max(
			first.level,
			first.x,
			first.y,
			last.x,
			last.y,
		)?);
	}
	Ok(strips)
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

	/// Reads up to `CHUNKS_AHEAD` chunks ahead of the `CHUNKS_SHADING` being
	/// shaded, each in a task of its own so that it goes on while the stream
	/// waits for shading.
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let chunks: Vec<TileBBox> = bbox.iter_grid(self.chunk_tiles_per_side()).collect();
		let jobs: Vec<(TileBBox, Option<TileBBox>)> = chunks
			.iter()
			.enumerate()
			.map(|(i, chunk)| (*chunk, chunks.get(i + 1).copied()))
			.collect();

		let reader = self.clone();
		let shader = self.clone();
		let tiles = stream::iter(jobs)
			// One chunk after the other, so that a stream waiting for a permit
			// holds permits only of chunks before the one it waits for.
			.then(move |(chunk, next)| {
				let this = reader.clone();
				async move {
					let permit = Arc::clone(&this.inner.chunks).acquire_owned().await;
					let reading = this.start_reading(chunk);
					let read = versatiles_core::stage::spawn(
						CancelToken::current().scope(async move { this.prefetch(chunk, next, reading).await }),
					);
					(chunk, permit, read)
				}
			})
			.map(|(chunk, permit, read)| async move { (chunk, permit, read.await) })
			.buffered(CHUNKS_AHEAD)
			.map(move |(chunk, permit, read)| {
				let this = shader.clone();
				async move {
					// Held until the chunk is shaded, so that its heights stay in
					// the cache until they are used. The semaphore is never closed.
					let Ok(_permit) = permit else {
						return Vec::new();
					};
					let error = match read {
						Ok(Ok(())) => return this.shade_chunk(chunk).await,
						Ok(Err(error)) => error,
						Err(error) => anyhow!("the reading task failed: {error}"),
					};
					this
						.inner
						.runtime
						.record_error(&format!("dem_hillshade reading {chunk:?}"), &error);
					Vec::new()
				}
			})
			.buffer_unordered(CHUNKS_SHADING)
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
	use std::sync::atomic::{AtomicU32, Ordering};

	use futures::TryStreamExt;
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
			"ramp",
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
		let pixel = WORLD_SIZE / f64::from(1u32 << LEVEL) / 256.0;
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

	/// Tile side of the sources that count their reads, small so that a test
	/// can fill the cache cheaply.
	const COUNTED_SIZE: u32 = 64;

	/// Chunks along each side of the area of `Counted`.
	const COUNTED_CHUNKS: u32 = 4;

	/// What a `Counted` source has seen.
	#[derive(Debug)]
	struct Counts {
		/// Tiles handed out.
		reads: AtomicU32,
		/// Requests open now, and the most at once.
		open: AtomicU32,
		widest: AtomicU32,
	}

	impl Counts {
		const fn new() -> Self {
			Counts {
				reads: AtomicU32::new(0),
				open: AtomicU32::new(0),
				widest: AtomicU32::new(0),
			}
		}
	}

	/// A source of 4 × 4 chunks of tiles that counts what it is asked for.
	/// Each request takes a moment, as with a remote source.
	#[derive(Debug)]
	struct Counted {
		inner: DummyImageSource,
		counts: &'static Counts,
	}

	impl Counted {
		fn bbox() -> TileBBox {
			let side = COUNTED_CHUNKS * CHUNK_TILES_AT_256;
			TileBBox::from_min_and_max(8, 64, 64, 64 + side - 1, 64 + side - 1).unwrap()
		}

		/// The hillshade of this source, with a cache that holds the chunks
		/// held and one more: about 2.3 times less than they need together.
		async fn hillshade(counts: &'static Counts) -> Result<Operation> {
			let mut inner = DummyImageSource::new(
				move |coord| {
					counts.reads.fetch_add(1, Ordering::Relaxed);
					let mut image = RgbImage::new(COUNTED_SIZE, COUNTED_SIZE);
					for (x, y, pixel) in image.enumerate_pixels_mut() {
						pixel.0 = encode(DemEncoding::Terrarium, f64::from(x + y + coord.x));
					}
					Tile::from_image(DynamicImage::ImageRgb8(image), TileFormat::PNG).ok()
				},
				TileFormat::PNG,
				Some(TilePyramid::from_tile_coords(Self::bbox().iter_coords())),
			)?;
			inner.tilejson_mut().tile_schema = Some(super::super::encoding::to_tile_schema(DemEncoding::Terrarium));

			let chunk_bytes = u64::from(CHUNK_TILES_AT_256 * CHUNK_TILES_AT_256 * COUNTED_SIZE * COUNTED_SIZE * 4);
			let op = Operation::build_with_cache(
				VPLNode::try_from_str("dem_hillshade")?,
				Box::new(Counted { inner, counts }),
				&PipelineFactory::new_dummy(),
				chunk_bytes * (CHUNKS_HELD as u64 + 1),
			)
			.await?;
			// Building measures the tile size, which the source does not declare.
			counts.reads.store(0, Ordering::Relaxed);
			counts.widest.store(0, Ordering::Relaxed);
			Ok(op)
		}
	}

	#[async_trait]
	impl TileSource for Counted {
		fn source_type(&self) -> Arc<SourceType> {
			self.inner.source_type()
		}
		fn metadata(&self) -> &TileSourceMetadata {
			self.inner.metadata()
		}
		fn tilejson(&self) -> &TileJSON {
			self.inner.tilejson()
		}
		async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
			let open = self.counts.open.fetch_add(1, Ordering::Relaxed) + 1;
			self.counts.widest.fetch_max(open, Ordering::Relaxed);
			tokio::time::sleep(std::time::Duration::from_millis(10)).await;
			let tiles = self.inner.tile_stream(bbox).await?.to_vec().await;
			self.counts.open.fetch_sub(1, Ordering::Relaxed);
			Ok(TileStream::from_vec(tiles))
		}
	}

	/// A stream reads each tile once, and at most the row below each chunk a
	/// second time, ahead of the chunk below: along each of 3 rows of chunks,
	/// 17 + 18 + 18 + 17 tiles, cut off where the source ends. Fewer where the
	/// chunk below is being read already. Reading each chunk with its ring, as
	/// before #310, read 4,900 tiles.
	#[tokio::test]
	async fn a_stream_reads_each_tile_once_and_at_most_a_row_per_chunk_again() -> Result<()> {
		static COUNTS: Counts = Counts::new();
		let op = Counted::hillshade(&COUNTS).await?;

		let tiles = op.tile_stream(Counted::bbox()).await?.to_vec().await;
		assert_eq!(tiles.len() as u64, Counted::bbox().count_tiles());
		let reads = COUNTS.reads.load(Ordering::Relaxed);
		assert!((64 * 64..=64 * 64 + 3 * 70).contains(&reads), "{reads} reads");
		Ok(())
	}

	/// Many streams at once, as a writer buffering small blocks opens them,
	/// read no more together than reading each chunk with its ring would, and
	/// their chunks still fit the cache. Before they shared the chunks held,
	/// such streams read about twice as much.
	#[tokio::test(flavor = "multi_thread")]
	async fn streams_opened_at_once_read_no_more_than_every_ring() -> Result<()> {
		static COUNTS: Counts = Counts::new();
		let op = Counted::hillshade(&COUNTS).await?;

		let blocks: Vec<TileBBox> = Counted::bbox().iter_grid(CHUNK_TILES_AT_256).collect();
		let counts: Vec<usize> = stream::iter(blocks)
			.map(|block| {
				let op = op.clone();
				async move { Ok::<_, anyhow::Error>(op.tile_stream(block).await?.to_vec().await.len()) }
			})
			.buffer_unordered(16)
			.try_collect()
			.await?;
		assert_eq!(counts.iter().sum::<usize>() as u64, Counted::bbox().count_tiles());
		let reads = COUNTS.reads.load(Ordering::Relaxed);
		assert!((64 * 64..=70 * 70).contains(&reads), "{reads} reads");
		Ok(())
	}

	/// Chunks are read ahead of the ones being shaded, so that a remote source
	/// is read while the cores shade (#310). Before, two chunks were read and
	/// then shaded, one pair after the other.
	#[tokio::test(flavor = "multi_thread")]
	async fn chunks_are_read_ahead_of_shading() -> Result<()> {
		static COUNTS: Counts = Counts::new();
		let op = Counted::hillshade(&COUNTS).await?;

		op.tile_stream(Counted::bbox()).await?.to_vec().await;
		let widest = COUNTS.widest.load(Ordering::Relaxed);
		assert!(
			usize::try_from(widest)? >= CHUNKS_AHEAD,
			"at most {widest} requests at once"
		);
		Ok(())
	}

	/// A source that does not declare its tile size gets chunks for the size
	/// of its tiles, measured, not for 256 px: Mapterhorn's planet declares
	/// none, and its chunks of 512 px tiles were four times too large.
	#[rstest::rstest]
	#[case::declared_512(512, Some(512), 8)]
	#[case::measured_512(512, None, 8)]
	#[case::measured_256(256, None, 16)]
	#[tokio::test]
	async fn chunks_follow_the_size_of_the_tiles(
		#[case] size: u32,
		#[case] declared: Option<u32>,
		#[case] side: u32,
	) -> Result<()> {
		let mut source = DummyImageSource::new(
			move |_| Tile::from_image(DynamicImage::ImageRgb8(RgbImage::new(size, size)), TileFormat::PNG).ok(),
			TileFormat::PNG,
			Some(TilePyramid::new_full_up_to(2)),
		)?;
		source.tilejson_mut().tile_schema = Some(super::super::encoding::to_tile_schema(DemEncoding::Terrarium));
		if let Some(declared) = declared {
			source.tilejson_mut().set_tile_size(declared)?;
		} else {
			assert!(source.tilejson().tile_size.is_none());
		}
		let op = hillshade(Ok(source), "dem_hillshade").await?;
		assert_eq!(op.chunk_tiles_per_side(), side);
		Ok(())
	}

	/// The ring of a chunk is read as its four sides, corners included.
	#[test]
	fn the_ring_of_a_chunk_comes_out_as_four_strips() -> Result<()> {
		let chunk = TileBBox::from_min_and_max(8, 16, 16, 31, 31)?;
		let mut strips: Vec<String> = strips(&Operation::ring(&chunk))?
			.into_iter()
			.map(|strip| {
				format!(
					"{}x{} at {},{}",
					strip.width(),
					strip.height(),
					strip.x_min().unwrap(),
					strip.y_min().unwrap()
				)
			})
			.collect();
		strips.sort();
		assert_eq!(
			strips,
			["18x1 at 15,15", "18x1 at 15,32", "1x16 at 15,16", "1x16 at 32,16"]
		);
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

	/// Flat ground has no shading, so what is left is the ramp's own colour,
	/// the same `dem_colorize` gives.
	#[rstest::rstest]
	#[case("0:000000,1000:FF8000")]
	#[case("0:FF000000,1000:0000FF")]
	#[tokio::test]
	async fn on_flat_ground_a_ramp_gives_the_colours_of_dem_colorize(#[case] ramp: &str) -> Result<()> {
		let op = hillshade(
			surface(DemEncoding::Mapbox, |_, _| 500.0),
			&format!("dem_hillshade ramp=\"{ramp}\""),
		)
		.await?;
		let expected = ColorRamp::try_from(ramp)?.color_at(500.0);
		let image = shaded_image(&op, ORIGIN, ORIGIN).await?;
		assert!(image.pixels().all(|p| p.0 == expected), "expected {expected:?}");
		Ok(())
	}

	/// One step gives what the two-step form gives, the shading in a layer over
	/// the colours in another, except that this one rounds only once.
	#[rstest::rstest]
	#[case::opaque("-1000:204060,1000:c8b464,50000:ffffff")]
	#[case::translucent("-1000:20406080,1000:c8b46440,50000:ffffffc0")]
	#[tokio::test]
	async fn a_ramp_matches_a_hillshade_stacked_over_the_colours(#[case] ramp: &str) -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let dem = "from_debug format=png | meta_update schema=\"dem/mapbox\"";
		let single = factory
			.operation_from_vpl(&format!("{dem} | dem_hillshade ramp=\"{ramp}\""))
			.await?;
		let stacked = factory
			.operation_from_vpl(&format!(
				"from_stacked_raster [ {dem} | dem_hillshade, {dem} | dem_colorize ramp=\"{ramp}\" ]"
			))
			.await?;

		let coord = TileCoord::new(3, 2, 1)?;
		let image = |tile: Option<Tile>| -> Result<RgbaImage> { Ok(tile.expect("a tile").into_image()?.to_rgba8()) };
		let single = image(single.tile(&coord).await?)?;
		let stacked = image(stacked.tile(&coord).await?)?;

		// Worth comparing only if the shading changes the picture.
		let colours = factory
			.operation_from_vpl(&format!("{dem} | dem_colorize ramp=\"{ramp}\""))
			.await?;
		let colours = image(colours.tile(&coord).await?)?;
		let shaded = single.pixels().zip(colours.pixels()).filter(|(a, b)| a != b).count();
		assert!(shaded > 1000, "only {shaded} pixels are shaded");

		let worst = single
			.pixels()
			.zip(stacked.pixels())
			.flat_map(|(a, b)| (0..4).map(move |k| a.0[k].abs_diff(b.0[k])))
			.max();
		assert!(worst.is_some_and(|worst| worst <= 2), "off by {worst:?}");
		Ok(())
	}

	/// The stream takes the same path as a single tile, ramp included.
	#[tokio::test]
	async fn with_a_ramp_the_stream_gives_the_tiles_one_at_a_time_would() -> Result<()> {
		let vpl = "dem_hillshade ramp=default";
		let meters = |x: f64, y: f64| (x * 0.05).sin() * 300.0 + y;
		let op = hillshade(surface(DemEncoding::Mapbox, meters), vpl).await?;
		let fresh = hillshade(surface(DemEncoding::Mapbox, meters), vpl).await?;
		let bbox = TileBBox::from_min_and_max(LEVEL, ORIGIN - 1, ORIGIN - 1, ORIGIN + 1, ORIGIN + 1)?;
		let streamed = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(streamed.len(), 9);
		for (coord, tile) in streamed {
			let single = fresh.tile(&coord).await?.expect("a tile").into_image()?;
			assert_eq!(tile.into_image()?.as_bytes(), single.as_bytes(), "tile {coord:?}");
		}
		Ok(())
	}

	/// The ramp decides RGB or RGBA and whether a JPEG source stays JPEG, as in
	/// `dem_colorize`; without one, nothing changes.
	#[rstest::rstest]
	#[case("dem_hillshade", TileSchema::RasterRGBA, TileFormat::PNG, false)]
	#[case("dem_hillshade ramp=default", TileSchema::RasterRGB, TileFormat::JPG, true)]
	#[case("dem_hillshade ramp=\"0:204060\"", TileSchema::RasterRGB, TileFormat::JPG, true)]
	#[case("dem_hillshade ramp=\"0:20406080\"", TileSchema::RasterRGBA, TileFormat::PNG, false)]
	#[tokio::test]
	async fn the_ramp_decides_rgb_or_rgba_and_whether_jpeg_stays(
		#[case] vpl: &str,
		#[case] schema: TileSchema,
		#[case] format: TileFormat,
		#[case] rgb: bool,
	) -> Result<()> {
		let source = DummyImageSource::from_color(&[1, 134, 160], 256, TileFormat::JPG, None)?;
		let vpl = format!("{vpl} encoding=mapbox");
		let op = Operation::build(
			VPLNode::try_from_str(&vpl)?,
			Box::new(source),
			&PipelineFactory::new_dummy(),
		)
		.await?;
		assert_eq!(op.tilejson().tile_schema, Some(schema));
		assert_eq!(*op.metadata().tile_format(), format);

		let tile = op.tile(&TileCoord::new(3, 2, 1)?).await?.expect("a tile");
		assert_eq!(matches!(tile.into_image()?, DynamicImage::ImageRgb8(_)), rgb);
		Ok(())
	}

	#[tokio::test]
	async fn a_bad_ramp_is_refused_by_check_and_build_alike() {
		let factory = PipelineFactory::new_dummy();
		let vpl =
			"from_debug format=png | meta_update schema=\"dem/mapbox\" | dem_hillshade ramp=\"100:000000,0:FFFFFF\"";
		let error = factory.operation_from_vpl(vpl).await.unwrap_err();
		assert!(format!("{error:#}").contains("ascend"), "{error:#}");

		let problems = factory.check(&crate::vpl::parse_vpl(vpl).unwrap());
		assert_eq!(problems.len(), 1, "{problems:?}");
		assert!(problems[0].message.contains("ascend"), "{problems:?}");
	}

	/// Refused where the value is decoded, so `check` says so too, without
	/// building anything, and the range a form offers is the one enforced.
	#[tokio::test]
	async fn an_exaggeration_outside_0_to_1_is_refused_by_check_and_build_alike() {
		let factory = PipelineFactory::new_dummy();
		for value in ["-0.1", "1.5", "NaN"] {
			let vpl =
				format!("from_debug format=png | meta_update schema=\"dem/mapbox\" | dem_hillshade exaggeration={value}");
			let error = factory.operation_from_vpl(&vpl).await.unwrap_err();
			assert!(format!("{error:#}").contains("between 0.0 and 1.0"), "{error:#}");

			let problems = factory.check(&crate::vpl::parse_vpl(&vpl).unwrap());
			assert_eq!(problems.len(), 1, "{problems:?}");
			assert!(problems[0].message.contains("between 0.0 and 1.0"), "{problems:?}");
		}
	}

	#[test]
	fn exaggeration_describes_the_range_it_enforces() {
		let bounds = Exaggeration::bounds();
		assert_eq!((bounds.min, bounds.max), (Some(0.0), Some(1.0)));
		assert!(bounds.min_inclusive && bounds.max_inclusive);
		assert!(!bounds.integer);
		for value in ["0", "1"] {
			assert!(Exaggeration::try_from(value).is_ok(), "{value}");
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
