//! Filling gaps: every tile the source lacks is created from the nearest tile
//! it has at a lower zoom level.
//!
//! Three questions are kept apart here, because they cost very different
//! amounts:
//!
//! - *Where can a tile be created at all?* Answered once, when the operation is
//!   built, as a pyramid ([`FillOverscale::fill`]).
//! - *Which tile would it be created from, and would it be empty?* Answered
//!   from the source's index and from [`Mask`]s, without scaling anything
//!   ([`FillOverscale::resolve`]).
//! - *What does it look like?* Only asked for tiles that passed the first two.

use std::{
	collections::{HashMap, HashSet},
	fmt::Debug,
	sync::{Arc, Mutex},
};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt, stream};
use moka::future::Cache;
use versatiles_container::{SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{
	ConcurrencyLimits, GeoBBox, MAX_ZOOM_LEVEL, TileBBox, TileCoord, TileCover, TileFormat, TileJSON, TilePyramid,
	TileStream,
};
use versatiles_image::DynamicImage;

use super::{mask::Mask, scale::Scaling};

/// A request for more tiles than this along one edge is answered box by box,
/// so that what has to be remembered about a box stays small.
const MAX_BOX_SIZE: u32 = 256;

/// How many tiles of the source's lowest levels are looked at when the
/// operation is built, to cut the area tiles are created in down to where
/// those tiles have content.
const MAX_MASKED_TILES: u64 = 64;

/// How many tiles are created from one source tile in one go. Small, because
/// every one of them is a decoded image until something encodes it.
const TILES_PER_TASK: usize = 16;

/// Creates every tile its source lacks, up to `level_max`, from the nearest
/// tile the source has at a lower zoom level. Tiles the source has are passed
/// on untouched.
#[derive(Clone)]
pub struct FillOverscale {
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
	source: SharedTileSource,
	/// The tiles the source has.
	native: Arc<TilePyramid>,
	/// Where a tile may be created, per level. Decided when the operation is
	/// built; whether a tile there really has content is decided per tile.
	fill: Arc<TilePyramid>,
	/// The source's lowest level. Nothing is created at or below it.
	level_min: u8,
	/// The highest level tiles are created at.
	level_max: u8,
	scaling: Scaling,
	/// Decoded source tiles, for scaling. `None` is a tile the source lacks.
	images: Cache<TileCoord, Option<Arc<DynamicImage>>>,
	/// Where source tiles have content. `None` is a tile the source lacks.
	masks: Cache<TileCoord, Option<Arc<Mask>>>,
}

impl Debug for FillOverscale {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("FillOverscale")
			.field("metadata", &self.metadata)
			.field("source", &self.source)
			.field("level_min", &self.level_min)
			.field("level_max", &self.level_max)
			.field("scaling", &self.scaling)
			.finish_non_exhaustive()
	}
}

impl FillOverscale {
	/// `level_max` defaults to the source's highest level.
	pub async fn new(source: Box<dyn TileSource>, level_max: Option<u8>) -> Result<Self> {
		let source: SharedTileSource = Arc::from(source);
		let native = source.tile_pyramid().await?;
		let level_min = native.level_min().context("source pyramid is empty")?;
		let level_top = native.level_max().context("source pyramid is empty")?;
		let level_max = level_max.unwrap_or(level_top).min(MAX_ZOOM_LEVEL);
		let scaling = Scaling::for_source(source.tilejson())?;

		let mut operation = Self {
			metadata: source.metadata().clone(),
			tilejson: source.tilejson().clone(),
			source,
			native,
			fill: Arc::new(TilePyramid::new_empty()),
			level_min,
			level_max,
			scaling,
			images: Cache::builder()
				.max_capacity(512 * 1024 * 1024)
				.weigher(|_, image: &Option<Arc<DynamicImage>>| {
					// A missing tile costs a cache entry, not nothing: weighed at
					// 8 bytes, 512 MB of them were 67 million entries.
					image.as_ref().map_or(256, |image| image.width() * image.height() * 4)
				})
				.build(),
			masks: Cache::builder()
				.max_capacity(64 * 1024 * 1024)
				.weigher(|_, mask: &Option<Arc<Mask>>| 256 + mask.as_ref().map_or(0, |mask| mask.weight()))
				.build(),
		};

		// Bounds the source declares are where its data is. Without them, the
		// pyramid has to say.
		let bounds = operation.tilejson.bounds.or_else(|| operation.native.geo_extent());
		let content = operation.content_of_lowest_levels().await?;
		let fill = fill_region(&operation.native, &content, bounds.as_ref(), level_min, level_max)?;

		let mut advertised = operation.native.as_ref().clone();
		for level in level_min.saturating_add(1)..=level_max {
			let cover = advertised.level_ref(level).union(fill.level_ref(level))?;
			advertised.set_level(cover);
		}
		operation.fill = Arc::new(fill);
		operation.metadata.set_tile_pyramid(advertised);
		operation.metadata.update_tilejson(&mut operation.tilejson);

		// Where tiles are created follows from the source's metadata, not from
		// a parameter, so say what was read from it.
		log::info!(
			"raster_overscale: creating missing tiles at levels {}-{level_max} from levels {level_min}-{level_top}, scaled as {}, inside {}{}",
			level_min.saturating_add(1),
			match scaling {
				Scaling::Color => "color",
				Scaling::Elevation => "elevation",
			},
			bounds.map_or_else(|| "no bounds".to_string(), |b| format!("{:?}", b.as_array())),
			match (content.first(), content.last()) {
				(Some((first, _)), Some((last, _))) =>
					format!(", cut to where the tiles of levels {first}-{last} have content"),
				_ => String::new(),
			}
		);

		Ok(operation)
	}

	/// For the lowest levels of the source, the area in which their tiles have
	/// content, each as tiles of the level at which one tile is one pixel.
	///
	/// One tile at level 0 covers the world, whatever it shows. Taken as it
	/// is, it would have tiles created all over the world from a source that
	/// holds one city, nearly all of them transparent. Its mask says where the
	/// city is, to the pixel, and a pixel is one tile several levels up. The
	/// same holds for the few tiles of the levels above it.
	///
	/// Stops at the first level whose tiles are all opaque: its tiles count in
	/// full, and nothing above it changes that. It stops as well once
	/// [`MAX_MASKED_TILES`] tiles have been looked at; by then the tiles are
	/// small, and the area they cover is close to the area of their content.
	async fn content_of_lowest_levels(&self) -> Result<Vec<(u8, TileCover)>> {
		let mut content = Vec::new();
		if self.never_transparent() {
			return Ok(content);
		}

		let mut budget = MAX_MASKED_TILES;
		let level_top = self.native.level_max().unwrap_or(self.level_min);
		for level in self.level_min..=level_top {
			let tiles = self.native.level_ref(level);
			if tiles.is_empty() {
				continue;
			}
			let Some(rest) = budget.checked_sub(tiles.count_tiles()) else {
				break;
			};
			budget = rest;

			let coords: Vec<TileCoord> = tiles.iter_coords().collect();
			let mut masks = Vec::new();
			for coord in &coords {
				if let Some(mask) = self.mask(*coord).await? {
					masks.push((*coord, mask));
				}
			}
			if masks.len() == coords.len() && masks.iter().all(|(_, mask)| **mask == Mask::Full) {
				break;
			}

			// The level at which one tile is one pixel of the finest mask. A
			// mask of an odd size does not map onto tiles, and counts as full.
			let pixel_depth = |mask: &Mask| match mask {
				Mask::Bits { width, height, .. } if width == height && width.is_power_of_two() => Some(width.ilog2()),
				_ => None,
			};
			let depth = masks
				.iter()
				.filter_map(|(_, mask)| pixel_depth(mask))
				.max()
				.unwrap_or(0);
			let pixel_level = u8::try_from(u32::from(level) + depth)
				.unwrap_or(MAX_ZOOM_LEVEL)
				.min(MAX_ZOOM_LEVEL);

			let mut cover = TileCover::new_empty(pixel_level)?;
			for (coord, mask) in masks {
				match &*mask {
					Mask::Bits { .. } if pixel_depth(&mask).is_none() => Mask::Full.add_content_to(coord, &mut cover)?,
					mask => mask.add_content_to(coord, &mut cover)?,
				}
			}
			content.push((level, cover));
		}
		Ok(content)
	}

	/// A format that cannot store transparency needs no look at its tiles.
	///
	/// Not the tile schema: `rgb` is also what a source is given when it
	/// declares none, so it does not say that the tiles have no alpha channel.
	fn never_transparent(&self) -> bool {
		*self.metadata.tile_format() == TileFormat::JPG
	}

	/// Whether any tile in `bbox` may have to be created.
	fn creates_in(&self, bbox: &TileBBox) -> bool {
		let level = bbox.level();
		level > self.level_min && level <= self.level_max && self.fill.level_ref(level).intersects_bbox(bbox)
	}

	/// The decoded tile of the source at `coord`, or `None` if it has none.
	async fn image(&self, coord: TileCoord) -> Result<Option<Arc<DynamicImage>>> {
		self
			.images
			.try_get_with(coord, async {
				let tile = self.source.tile(&coord).await?;
				tile.map(|tile| tile.into_image().map(Arc::new)).transpose()
			})
			.await
			.map_err(|error: Arc<anyhow::Error>| anyhow!("{error:#}"))
	}

	/// Where the tile of the source at `coord` has content, or `None` if the
	/// source has no such tile.
	///
	/// Reads the tile, which also settles whether it exists: an index can
	/// promise a tile that then does not come. It is decoded only if its
	/// header leaves the question open.
	async fn mask(&self, coord: TileCoord) -> Result<Option<Arc<Mask>>> {
		self
			.masks
			.try_get_with(coord, async {
				if let Some(image) = self.images.get(&coord).await {
					return Ok(image.map(|image| Arc::new(Mask::from_image(&image))));
				}
				let Some(mut tile) = self.source.tile(&coord).await? else {
					return Ok(None);
				};
				if self.never_transparent() || tile.is_opaque()? {
					return Ok(Some(Arc::new(Mask::Full)));
				}
				let image = Arc::new(tile.into_image()?);
				let mask = Mask::from_image(&image);
				self.images.insert(coord, Some(image)).await;
				Ok(Some(Arc::new(mask)))
			})
			.await
			.map_err(|error: Arc<anyhow::Error>| anyhow!("{error:#}"))
	}

	/// Cuts the area of `coord` out of the source tile at `source_coord` and
	/// scales it up to a tile.
	fn scale(&self, image: &DynamicImage, source_coord: TileCoord, coord: TileCoord) -> Result<Tile> {
		self
			.scaling
			.tile(image, source_coord, coord, *self.metadata.tile_format())
	}

	/// Creates the tile at `coord` from the nearest tile the source has below
	/// it, or returns `None` if there is none or it has no content there.
	async fn create(&self, coord: TileCoord) -> Result<Option<Tile>> {
		let region = coord.to_tile_bbox();
		let mut source_coord = coord;
		while source_coord.level > self.level_min {
			source_coord = source_coord.to_level_decreased()?;
			if !self.native.includes_coord(&source_coord) {
				continue;
			}

			// With the image in the cache, the mask is derived from it; the
			// other way round, an opaque tile would be read twice.
			let mask = if let Some(mask) = self.masks.get(&source_coord).await {
				mask
			} else {
				self.image(source_coord).await?;
				self.mask(source_coord).await?
			};
			let Some(mask) = mask else {
				continue;
			};
			if !mask.has_content(source_coord, &region) {
				return Ok(None);
			}
			let Some(image) = self.image(source_coord).await? else {
				continue;
			};
			return self.scale(&image, source_coord, coord).map(Some);
		}
		Ok(None)
	}

	/// Decides, for every tile in `bbox` that is not in `natives`, which source
	/// tile it is created from. Tiles that would be empty are left out.
	///
	/// Works upward one level at a time, on whole boxes of tiles: what is still
	/// open is scaled to the level below, and where the source has a tile
	/// there, its mask settles everything above it. A level the source has
	/// nothing at in this area is skipped without looking at a single tile, so
	/// a box far from any data is settled by one mask.
	async fn resolve(&self, bbox: TileBBox, natives: &HashSet<TileCoord>) -> Result<Vec<(TileCoord, TileCoord)>> {
		let mut plan = Vec::new();
		if !self.creates_in(&bbox) {
			return Ok(plan);
		}
		let level = bbox.level();
		let fill = self.fill.level_ref(level);

		// What still needs a source, as boxes of tiles at `level`.
		let mut open: Vec<TileBBox> = if natives.is_empty() {
			vec![bbox]
		} else {
			bbox
				.iter_coords()
				.filter(|coord| !natives.contains(coord) && fill.includes_coord(coord))
				.map(|coord| coord.to_tile_bbox())
				.collect()
		};

		for source_level in (self.level_min..level).rev() {
			if open.is_empty() {
				break;
			}
			let cover = self.native.level_ref(source_level);
			if cover.is_empty() || !cover.intersects_bbox(&bbox.at_level(source_level)) {
				continue;
			}

			let mut still_open = Vec::new();
			let mut candidates: Vec<(TileCoord, TileBBox)> = Vec::new();
			for region in open {
				let cells = region.at_level(source_level);
				if !cover.intersects_bbox(&cells) {
					still_open.push(region);
					continue;
				}
				let single = cells.count_tiles() == 1;
				for cell in cells.iter_coords() {
					let part = if single {
						region
					} else {
						cell.to_tile_bbox().at_level(level).intersection_bbox(&region)?
					};
					if part.is_empty() {
						continue;
					}
					if cover.includes_coord(&cell) {
						candidates.push((cell, part));
					} else {
						still_open.push(part);
					}
				}
			}

			let cells: HashSet<TileCoord> = candidates.iter().map(|(cell, _)| *cell).collect();
			let masks: HashMap<TileCoord, Option<Arc<Mask>>> = stream::iter(cells)
				.map(|cell| async move { self.mask(cell).await.map(|mask| (cell, mask)) })
				.buffer_unordered(ConcurrencyLimits::default().io_bound)
				.try_collect()
				.await?;

			for (cell, part) in candidates {
				let Some(Some(mask)) = masks.get(&cell) else {
					still_open.push(part);
					continue;
				};
				if !mask.has_content(cell, &part) {
					continue;
				}
				let full = **mask == Mask::Full;
				for coord in part.iter_coords() {
					if natives.contains(&coord) || !fill.includes_coord(&coord) {
						continue;
					}
					if full || mask.has_content(cell, &coord.to_tile_bbox()) {
						plan.push((coord, cell));
					}
				}
			}
			open = still_open;
		}

		Ok(plan)
	}

	/// The tiles of the source in `bbox`.
	async fn native_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		if self.native.intersects_bbox(&bbox) {
			self.source.tile_stream(bbox).await
		} else {
			Ok(TileStream::empty())
		}
	}

	/// The tiles in `bbox`: first those the source has, then those created.
	///
	/// Which tiles the source has is taken from what its stream hands out, not
	/// from its index: a source can list a tile it then does not deliver, and
	/// that tile is a gap like any other.
	async fn box_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let arrived = Arc::new(Mutex::new(HashSet::new()));
		let recorder = Arc::clone(&arrived);
		let natives = self.native_stream(bbox).await?.inspect(move |coord, _| {
			recorder.lock().expect("poisoned mutex").insert(coord);
		});

		let this = self.clone();
		let created = stream::once(async move {
			let natives = std::mem::take(&mut *arrived.lock().expect("poisoned mutex"));
			match this.created_stream(bbox, &natives).await {
				Ok(stream) => stream.inner,
				Err(error) => {
					log::error!("Error creating tiles in {bbox:?}: {error:#}");
					TileStream::empty().inner
				}
			}
		})
		.flatten();

		Ok(TileStream::from_stream(Box::pin(natives.inner.chain(created))))
	}

	/// The tiles that have to be created in `bbox`, given the ones the source
	/// delivered.
	async fn created_stream(&self, bbox: TileBBox, natives: &HashSet<TileCoord>) -> Result<TileStream<'static, Tile>> {
		let mut by_source: HashMap<TileCoord, Vec<TileCoord>> = HashMap::new();
		for (coord, source_coord) in self.resolve(bbox, natives).await? {
			by_source.entry(source_coord).or_default().push(coord);
		}
		let tasks: Vec<(TileCoord, Vec<TileCoord>)> = by_source
			.into_iter()
			.flat_map(|(source_coord, coords)| {
				coords
					.chunks(TILES_PER_TASK)
					.map(|chunk| (source_coord, chunk.to_vec()))
					.collect::<Vec<_>>()
			})
			.collect();

		let this = self.clone();
		let tiles = stream::iter(tasks)
			.map(move |(source_coord, coords)| {
				let this = this.clone();
				async move {
					let image = match this.image(source_coord).await {
						Ok(Some(image)) => image,
						Ok(None) => return Vec::new(),
						Err(error) => {
							log::error!("Error reading tile {source_coord:?}: {error:#}");
							return Vec::new();
						}
					};
					let scaled = tokio::task::spawn_blocking(move || {
						coords
							.into_iter()
							.filter_map(|coord| match this.scale(&image, source_coord, coord) {
								Ok(tile) => Some((coord, tile)),
								Err(error) => {
									log::error!("Error creating tile {coord:?}: {error:#}");
									None
								}
							})
							.collect::<Vec<_>>()
					});
					scaled.await.unwrap_or_default()
				}
			})
			.buffer_unordered(ConcurrencyLimits::default().cpu_bound)
			.flat_map(stream::iter);

		Ok(TileStream::from_stream(Box::pin(tiles)))
	}

	/// The coordinates of the tiles in `bbox`: those the source lists, and
	/// those that would be created.
	async fn box_coords(&self, bbox: TileBBox) -> Result<Vec<(TileCoord, ())>> {
		let mut natives = HashSet::new();
		if self.native.intersects_bbox(&bbox) {
			let mut stream = self.source.tile_coord_stream(bbox).await?;
			while let Some((coord, ())) = stream.next().await {
				natives.insert(coord);
			}
		}
		let created = self.resolve(bbox, &natives).await?;
		Ok(natives
			.into_iter()
			.chain(created.into_iter().map(|(coord, _)| coord))
			.map(|coord| (coord, ()))
			.collect())
	}
}

/// Where a tile may be created, per level from `level_min + 1` to `level_max`.
///
/// A tile can be created wherever the source has a tile at a lower level:
/// each level takes over the area of the levels below it. The levels in
/// `content` contribute the area of their *content*, which can be less than
/// their tiles; the others contribute their tiles. All of it is cut to the
/// tiles that reach into `bounds`.
///
/// More than is created in the end, never less: whether a tile has content is
/// decided by the nearest tile below it, and that one can be empty where a
/// lower one is not.
fn fill_region(
	native: &TilePyramid,
	content: &[(u8, TileCover)],
	bounds: Option<&GeoBBox>,
	level_min: u8,
	level_max: u8,
) -> Result<TilePyramid> {
	let mut fill = TilePyramid::new_empty();
	// The tiles of the levels below the current one that are not in `content`.
	let lowest = native.level_ref(level_min);
	let mut tiles_below: Option<TileCover> = (!lowest.is_empty()
		&& !content.iter().any(|(content_level, _)| *content_level == level_min))
	.then(|| lowest.clone());

	for level in level_min.saturating_add(1)..=level_max {
		let mut cover = TileCover::new_empty(level)?;
		for (_, content) in content.iter().filter(|(content_level, _)| *content_level < level) {
			cover = cover.union(&content.at_level(level))?;
		}
		if let Some(tiles_below) = &tiles_below {
			cover = cover.union(&tiles_below.at_level(level))?;
		}
		if let Some(bounds) = bounds {
			let reached = TileBBox::from_geo_bbox(level, bounds)?;
			cover = cover.intersection(&TileCover::from(reached))?;
		}
		fill.set_level(cover);

		let tiles = native.level_ref(level);
		if !tiles.is_empty() && !content.iter().any(|(content_level, _)| *content_level == level) {
			tiles_below = Some(match tiles_below {
				Some(below) => below.at_level(level).union(tiles)?,
				None => tiles.clone(),
			});
		}
	}
	Ok(fill)
}

#[async_trait]
impl TileSource for FillOverscale {
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("raster_overscale", self.source.source_type())
	}

	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		if self.native.includes_coord(coord)
			&& let Some(tile) = self.source.tile(coord).await?
		{
			return Ok(Some(tile));
		}
		if !self.creates_in(&coord.to_tile_bbox()) {
			return Ok(None);
		}
		self.create(*coord).await
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("raster_overscale::tile_stream {bbox:?}");

		if !self.creates_in(&bbox) {
			return self.native_stream(bbox).await;
		}
		if bbox.width() <= MAX_BOX_SIZE && bbox.height() <= MAX_BOX_SIZE {
			return self.box_stream(bbox).await;
		}

		// One box after the other: each is settled before the next is begun.
		let this = self.clone();
		let boxes: Vec<TileBBox> = bbox.iter_grid(MAX_BOX_SIZE).collect();
		let tiles = stream::iter(boxes)
			.then(move |cell| {
				let this = this.clone();
				async move {
					match this.box_stream(cell).await {
						Ok(stream) => stream.inner,
						Err(error) => {
							log::error!("Error reading tiles in {cell:?}: {error:#}");
							TileStream::empty().inner
						}
					}
				}
			})
			.flatten();
		Ok(TileStream::from_stream(Box::pin(tiles)))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		if !self.creates_in(&bbox) {
			return if self.native.intersects_bbox(&bbox) {
				self.source.tile_coord_stream(bbox).await
			} else {
				Ok(TileStream::empty())
			};
		}

		let this = self.clone();
		let boxes: Vec<TileBBox> = bbox.iter_grid(MAX_BOX_SIZE).collect();
		let coords = stream::iter(boxes)
			.then(move |cell| {
				let this = this.clone();
				async move {
					match this.box_coords(cell).await {
						Ok(coords) => coords,
						Err(error) => {
							log::error!("Error listing tiles in {cell:?}: {error:#}");
							Vec::new()
						}
					}
				}
			})
			.flat_map(stream::iter);
		Ok(TileStream::from_stream(Box::pin(coords)))
	}
}

#[cfg(test)]
#[expect(clippy::cast_possible_truncation, reason = "test data is built from literal values")]
mod tests {
	use versatiles_container::Traversal;
	use versatiles_core::TileCompression;
	use versatiles_image::DynamicImageTraitConvert;

	use super::*;

	// --- a source that says exactly what it has, and counts what is read ---

	#[derive(Debug)]
	struct TestSource {
		tiles: HashMap<TileCoord, Tile>,
		metadata: TileSourceMetadata,
		tilejson: TileJSON,
		reads: Arc<Mutex<HashMap<TileCoord, u32>>>,
	}

	impl TestSource {
		/// Tiles as a container hands them out: encoded, not decoded.
		fn new(format: TileFormat, tiles: Vec<(TileCoord, DynamicImage)>, bounds: Option<GeoBBox>) -> Self {
			let pyramid = TilePyramid::from_tile_coords(tiles.iter().map(|(coord, _)| *coord));
			let tiles = tiles
				.into_iter()
				.map(|(coord, image)| {
					let blob = image.to_blob(format, None, None).unwrap();
					(coord, Tile::from_blob(blob, TileCompression::Uncompressed, format))
				})
				.collect();
			let metadata = TileSourceMetadata::new(format, TileCompression::Uncompressed, Traversal::ANY, Some(pyramid));
			let mut tilejson = TileJSON {
				bounds,
				..TileJSON::default()
			};
			metadata.update_tilejson(&mut tilejson);
			Self {
				tiles,
				metadata,
				tilejson,
				reads: Arc::new(Mutex::new(HashMap::new())),
			}
		}

		fn read(&self, coord: &TileCoord) -> Option<Tile> {
			let tile = self.tiles.get(coord)?;
			*self.reads.lock().unwrap().entry(*coord).or_default() += 1;
			Some(tile.clone())
		}
	}

	#[async_trait]
	impl TileSource for TestSource {
		fn source_type(&self) -> Arc<SourceType> {
			SourceType::new_container("test source", "test")
		}

		fn metadata(&self) -> &TileSourceMetadata {
			&self.metadata
		}

		fn tilejson(&self) -> &TileJSON {
			&self.tilejson
		}

		async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
			Ok(self.read(coord))
		}

		async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
			let coords: Vec<TileCoord> = self.tiles.keys().filter(|c| c.level == bbox.level()).copied().collect();
			Ok(TileStream::from_vec(
				coords
					.into_iter()
					.filter(|coord| bbox.includes_coord(coord))
					.filter_map(|coord| self.read(&coord).map(|tile| (coord, tile)))
					.collect(),
			))
		}

		async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
			Ok(TileStream::from_vec(
				self
					.tiles
					.keys()
					.filter(|coord| coord.level == bbox.level() && bbox.includes_coord(coord))
					.map(|coord| (*coord, ()))
					.collect(),
			))
		}
	}

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	/// An opaque 8×8 tile with a gradient of its own, so that both the tile an
	/// upscale came from and the region cut out of it show.
	fn opaque(c: TileCoord) -> DynamicImage {
		let (level, x, y) = (u32::from(c.level), c.x, c.y);
		DynamicImage::from_fn(8, 8, |px, py| {
			[
				(level * 37 + px * 9) as u8,
				(x * 29 + py * 11) as u8,
				(y * 31 + px + py) as u8,
			]
		})
	}

	/// Like [`opaque`], but with content only in the pixels `x0..=x1`, `y0..=y1`.
	fn partial(c: TileCoord, [x0, y0, x1, y1]: [u32; 4]) -> DynamicImage {
		let (level, x, y) = (u32::from(c.level), c.x, c.y);
		DynamicImage::from_fn(8, 8, |px, py| {
			let inside = (x0..=x1).contains(&px) && (y0..=y1).contains(&py);
			[
				(level * 37 + px * 9) as u8,
				(x * 29 + py * 11) as u8,
				(y * 31 + px + py) as u8,
				if inside { 255 } else { 0 },
			]
		})
	}

	fn level_tiles(level: u8, x: std::ops::RangeInclusive<u32>, y: std::ops::RangeInclusive<u32>) -> Vec<TileCoord> {
		x.flat_map(|x| y.clone().map(move |y| coord(level, x, y))).collect()
	}

	/// Opaque, worldwide to level 2, one region at level 3 and a smaller one
	/// inside it at level 4: the shape of imagery with aerial photos on top.
	fn satellite_like() -> TestSource {
		let mut coords = Vec::new();
		coords.extend(level_tiles(0, 0..=0, 0..=0));
		coords.extend(level_tiles(1, 0..=1, 0..=1));
		coords.extend(level_tiles(2, 0..=3, 0..=3));
		coords.extend(level_tiles(3, 2..=3, 2..=3));
		coords.extend(level_tiles(4, 4..=5, 4..=5));
		TestSource::new(
			TileFormat::PNG,
			coords.into_iter().map(|c| (c, opaque(c))).collect(),
			None,
		)
	}

	/// One small place, at every level from 0 to 3, on transparent ground: the
	/// tile `3/5/2` is all content, and each tile below it has content exactly
	/// where that tile lies. With `bounds`, or with the bounds of its pyramid.
	fn single_place(bounds: Option<GeoBBox>) -> TestSource {
		TestSource::new(
			TileFormat::PNG,
			vec![
				(coord(0, 0, 0), partial(coord(0, 0, 0), [5, 2, 5, 2])),
				(coord(1, 1, 0), partial(coord(1, 1, 0), [2, 4, 3, 5])),
				(coord(2, 2, 1), partial(coord(2, 2, 1), [4, 0, 7, 3])),
				(coord(3, 5, 2), opaque(coord(3, 5, 2))),
			],
			bounds,
		)
	}

	fn world() -> GeoBBox {
		GeoBBox::new(-180.0, -85.051_128_779_806_59, 180.0, 85.051_128_779_806_59).unwrap()
	}

	async fn fill(source: TestSource, level_max: Option<u8>) -> FillOverscale {
		FillOverscale::new(Box::new(source), level_max).await.unwrap()
	}

	async fn sorted_coords<T: Send + 'static>(stream: TileStream<'static, T>) -> Vec<(u8, u32, u32)> {
		let mut coords: Vec<_> = stream
			.to_vec()
			.await
			.into_iter()
			.map(|(c, _)| (c.level, c.x, c.y))
			.collect();
		coords.sort_unstable();
		coords
	}

	/// What scaling the area of `coord` out of the source's tile at `from` gives.
	fn expected(source: &TestSource, from: TileCoord, coord: TileCoord) -> DynamicImage {
		let image = source.tiles[&from].clone().into_image().unwrap();
		Scaling::Color.extract(&image, from, coord).unwrap()
	}

	// --- tiles the source has ---

	#[tokio::test]
	async fn a_tile_the_source_has_is_passed_on_with_its_bytes() -> Result<()> {
		let source = satellite_like();
		let original = source.tiles[&coord(4, 4, 5)].clone();
		let op = fill(source, None).await;

		let tile = op.tile(&coord(4, 4, 5)).await?.unwrap();
		assert!(tile.has_blob() && !tile.has_content(), "passed on, not decoded");
		assert_eq!(
			tile.into_blob(&TileCompression::Uncompressed)?,
			original.into_blob(&TileCompression::Uncompressed)?
		);
		Ok(())
	}

	// --- tiles that are created ---

	#[rstest::rstest]
	// Next to the tiles at level 4, inside the region of level 3.
	#[case(coord(4, 6, 6), coord(3, 3, 3))]
	// Outside that region: level 3 has nothing here, level 2 has.
	#[case(coord(4, 15, 0), coord(2, 3, 0))]
	// A gap at level 3 itself.
	#[case(coord(3, 7, 7), coord(2, 3, 3))]
	#[tokio::test]
	async fn a_missing_tile_is_created_from_the_nearest_level_below(
		#[case] missing: TileCoord,
		#[case] from: TileCoord,
	) -> Result<()> {
		let source = satellite_like();
		let image = expected(&source, from, missing);
		let op = fill(source, None).await;

		let tile = op.tile(&missing).await?.expect("the gap should be filled");
		assert!(!tile.has_blob(), "created, so not encoded yet");
		assert_eq!(tile.into_image()?.as_bytes(), image.as_bytes());
		Ok(())
	}

	#[tokio::test]
	async fn every_level_is_complete_up_to_the_highest_of_the_source() -> Result<()> {
		let op = fill(satellite_like(), None).await;
		let pyramid = op.metadata().tile_pyramid().unwrap();

		for (level, count) in [(0u8, 1usize), (1, 4), (2, 16), (3, 64), (4, 256), (5, 0)] {
			let bbox = TileBBox::new_full(level)?;
			let tiles = sorted_coords(op.tile_stream(bbox).await?).await;
			let coords = sorted_coords(op.tile_coord_stream(bbox).await?).await;
			assert_eq!(tiles.len(), count, "tiles at level {level}");
			assert_eq!(tiles, coords, "tile and coordinate stream at level {level}");
			assert_eq!(
				pyramid.level_ref(level).count_tiles(),
				count as u64,
				"pyramid at level {level}"
			);
		}
		assert_eq!(op.tilejson().zoom_max(), Some(4));
		Ok(())
	}

	#[tokio::test]
	async fn level_max_can_go_beyond_the_source() -> Result<()> {
		let op = fill(satellite_like(), Some(6)).await;

		assert_eq!(op.metadata().tile_pyramid().unwrap().level_max(), Some(6));
		let bbox = TileBBox::from_min_and_max(6, 10, 10, 13, 13)?;
		assert_eq!(op.tile_stream(bbox).await?.to_vec().await.len(), 16);
		assert!(op.tile(&coord(7, 0, 0)).await?.is_none());
		Ok(())
	}

	/// Below the highest level of the source, `level_max` stops the creating
	/// and nothing else: the tiles the source has above it are still there.
	#[tokio::test]
	async fn level_max_below_the_source_keeps_its_tiles() -> Result<()> {
		let op = fill(satellite_like(), Some(3)).await;

		assert!(op.tile(&coord(3, 7, 7)).await?.is_some(), "created at level 3");
		assert!(
			op.tile(&coord(4, 4, 4)).await?.is_some(),
			"the source's tile at level 4"
		);
		assert!(op.tile(&coord(4, 6, 6)).await?.is_none(), "nothing created at level 4");

		let tiles = sorted_coords(op.tile_stream(TileBBox::new_full(4)?).await?).await;
		assert_eq!(tiles, [(4, 4, 4), (4, 4, 5), (4, 5, 4), (4, 5, 5)]);
		Ok(())
	}

	/// Three levels above an 8 pixel tile, a tile is one pixel; beyond that it
	/// is a part of one, and comes out in a single color.
	#[tokio::test]
	async fn far_above_its_source_a_tile_is_one_color() -> Result<()> {
		let source = TestSource::new(TileFormat::PNG, vec![(coord(0, 0, 0), opaque(coord(0, 0, 0)))], None);
		let op = fill(source, Some(12)).await;

		let image = op.tile(&coord(12, 2500, 1300)).await?.unwrap().into_image()?;
		let first = &image.as_bytes()[0..3];
		assert!(image.as_bytes().chunks(3).all(|pixel| pixel == first));
		Ok(())
	}

	// --- where nothing is created ---

	/// The pyramid of this source is one tile per level, and its lowest one is
	/// the whole world. Its bounds are derived from the highest level, which
	/// keeps tiles from being created anywhere else.
	#[tokio::test]
	async fn nothing_is_created_outside_the_bounds() -> Result<()> {
		let op = fill(single_place(None), Some(5)).await;
		let pyramid = op.metadata().tile_pyramid().unwrap();

		assert_eq!(pyramid.level_ref(4).count_tiles(), 4);
		assert_eq!(pyramid.level_ref(5).count_tiles(), 16);
		assert_eq!(op.tile_stream(TileBBox::new_full(5)?).await?.to_vec().await.len(), 16);
		assert!(op.tile(&coord(5, 0, 0)).await?.is_none());
		assert!(op.tile(&coord(5, 20, 8)).await?.is_some());
		Ok(())
	}

	/// With bounds that say "the whole world", the transparency has to do it:
	/// the tile at level 0 has content in one pixel, and that is where tiles
	/// are created.
	#[tokio::test]
	async fn nothing_is_created_where_the_source_is_transparent() -> Result<()> {
		let source = single_place(Some(world()));
		let reads = Arc::clone(&source.reads);
		let op = fill(source, Some(5)).await;

		// The pyramid is cut to the content already.
		let pyramid = op.metadata().tile_pyramid().unwrap();
		assert_eq!(pyramid.level_ref(5).count_tiles(), 16);

		for level in 1..=5u8 {
			let bbox = TileBBox::new_full(level)?;
			let tiles = sorted_coords(op.tile_stream(bbox).await?).await;
			let coords = sorted_coords(op.tile_coord_stream(bbox).await?).await;
			assert_eq!(tiles, coords, "level {level}");
			// Exactly the tiles over `3/5/2`, or the one that contains it.
			let shift = i32::from(level) - 3;
			let expected = if shift <= 0 { 1 } else { 1usize << (2 * shift) };
			assert_eq!(tiles.len(), expected, "level {level}");
		}

		// Far away, and right next to the content.
		for missing in [coord(5, 0, 31), coord(4, 12, 4), coord(4, 9, 4)] {
			assert!(op.tile(&missing).await?.is_none(), "{missing:?}");
		}

		// Through all of that, the world tile was read once, when the
		// operation was built: what it shows is remembered as a mask.
		assert_eq!(reads.lock().unwrap()[&coord(0, 0, 0)], 1);
		Ok(())
	}

	/// A tile that is transparent in the source is a tile the source has.
	#[tokio::test]
	async fn a_transparent_tile_of_the_source_is_passed_on() -> Result<()> {
		let empty = partial(coord(1, 0, 0), [9, 9, 9, 9]);
		// With the bounds of its pyramid, this source would end at the area of
		// its one tile at level 1.
		let source = TestSource::new(
			TileFormat::PNG,
			vec![(coord(0, 0, 0), opaque(coord(0, 0, 0))), (coord(1, 0, 0), empty)],
			Some(world()),
		);
		let reads = Arc::clone(&source.reads);
		let op = fill(source, Some(2)).await;

		assert!(op.tile(&coord(1, 0, 0)).await?.is_some());
		// And nothing is created from it: it is the nearest tile below, and
		// it is empty there.
		assert!(op.tile(&coord(2, 0, 0)).await?.is_none());

		// Asking again does not read it again: its mask is remembered.
		let before = reads.lock().unwrap()[&coord(1, 0, 0)];
		for missing in [coord(2, 0, 0), coord(2, 1, 0), coord(2, 0, 1), coord(2, 1, 1)] {
			assert!(op.tile(&missing).await?.is_none(), "{missing:?}");
		}
		assert_eq!(op.tile_stream(TileBBox::new_full(2)?).await?.to_vec().await.len(), 12);
		assert_eq!(reads.lock().unwrap()[&coord(1, 0, 0)], before);
		// Next to it, the world tile is the nearest.
		assert!(op.tile(&coord(2, 2, 0)).await?.is_some());
		Ok(())
	}

	// --- behind a stack of sources ---

	/// The setup the operation is meant for: imagery of the whole world at
	/// low levels, and of one region at high levels on top, blended by
	/// `from_stacked_raster`. Where only the world layer has anything, the
	/// stack emits no tile above that layer's own levels, and lists the
	/// coordinate all the same.
	#[tokio::test]
	async fn behind_a_stack_it_fills_what_the_stack_leaves_out() -> Result<()> {
		use versatiles_image::DynamicImageTraitOperation;

		let factory = crate::PipelineFactory::new_dummy();
		let stack = r"from_stacked_raster auto_overscale=true [
			from_color color=FF0000 format=png tile_size=256 | filter level_max=5 bbox=[10,10,40,40],
			from_color color=0000FF format=png tile_size=256 | filter level_max=2
		]";
		let color_at = async |vpl: String, level: u8, x: u32, y: u32| -> Result<Option<Vec<u8>>> {
			let op = factory.operation_from_vpl(&vpl).await?;
			let mut tiles = op.tile_stream(coord(level, x, y).to_tile_bbox()).await?.to_vec().await;
			Ok(tiles.pop().map(|(_, tile)| tile.into_image().unwrap().average_color()))
		};
		// At level 5, inside the region and far away from it.
		let inside = (5, 18, 13);
		let outside = (5, 3, 28);

		// The stack by itself: the region, and a hole everywhere else.
		assert_eq!(
			color_at(stack.to_string(), inside.0, inside.1, inside.2).await?,
			Some(vec![255, 0, 0])
		);
		assert_eq!(
			color_at(stack.to_string(), outside.0, outside.1, outside.2).await?,
			None
		);

		// With the operation behind it: the region as it was, the world layer
		// scaled up around it.
		let filled = format!("{stack} | raster_overscale");
		assert_eq!(
			color_at(filled.clone(), inside.0, inside.1, inside.2).await?,
			Some(vec![255, 0, 0])
		);
		assert_eq!(
			color_at(filled.clone(), outside.0, outside.1, outside.2).await?,
			Some(vec![0, 0, 255])
		);

		let op = factory.operation_from_vpl(&filled).await?;
		for level in 3..=5u8 {
			let bbox = TileBBox::new_full(level)?;
			let count = 1usize << (2 * level);
			assert_eq!(op.tile_stream(bbox).await?.to_vec().await.len(), count, "level {level}");
			assert_eq!(
				op.tile_coord_stream(bbox).await?.to_vec().await.len(),
				count,
				"level {level}"
			);
		}
		Ok(())
	}

	// --- a source that lists tiles it does not deliver ---

	/// `DummyImageSource` reports every coordinate of its pyramid, and this
	/// one hands out nothing at level 2. A stack of sources behaves like that
	/// where all it has are upscales.
	#[tokio::test]
	async fn a_tile_that_was_listed_but_not_delivered_is_created() -> Result<()> {
		use crate::helpers::dummy_image_source::DummyImageSource;

		let source = DummyImageSource::new(
			|c| (c.level != 2).then(|| Tile::from_image(opaque(*c), TileFormat::PNG).unwrap()),
			TileFormat::PNG,
			Some(TilePyramid::new_full_up_to(2)),
		)?;
		let op = FillOverscale::new(Box::new(source), None).await?;

		let tiles = op.tile_stream(TileBBox::new_full(2)?).await?.to_vec().await;
		assert_eq!(tiles.len(), 16);
		assert!(op.tile(&coord(2, 3, 3)).await?.is_some());
		Ok(())
	}
}
