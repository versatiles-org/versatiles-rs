use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

use anyhow::{Result, ensure};
use dashmap::DashMap;
use imageproc::image::{DynamicImage, GenericImage};
use versatiles_container::{
	Tile, TileSource, TileSourceMetadata, TileStreamErrorExt, TilesRuntime, Traversal, TraversalOrder,
};
use versatiles_core::{Blob, TileBBox, TileBBoxMap, TileCoord, TileJSON, TileStream};
use versatiles_derive::context;
use versatiles_image::traits::DynamicImageTraitInfo;

static BLOCK_TILE_COUNT: u32 = 16;

/// Tile size assumed when a source declares none.
const DEFAULT_TILE_SIZE: u32 = 512;

/// Scaling function type used to downscale tiles by a factor of 2.
pub type ScaleDownFn = Arc<dyn Fn(&DynamicImage) -> Result<DynamicImage> + Send + Sync>;

/// Shared overview core that generates lower-zoom tiles by downscaling from a base zoom level.
///
/// The actual downscaling algorithm is provided via `scale_fn`, allowing reuse for both
/// standard raster (channel-wise averaging) and DEM (24-bit raw value averaging) tiles.
pub struct OverviewCore {
	pub runtime: TilesRuntime,
	pub metadata: TileSourceMetadata,
	pub source: Box<dyn TileSource>,
	pub tilejson: TileJSON,
	pub level_base: u8,
	pub tile_size: u32,
	#[expect(
		clippy::type_complexity,
		reason = "a coord-keyed cache of scaled-down tiles; a type alias would only move the signature"
	)]
	pub cache: Arc<DashMap<TileCoord, Vec<(TileCoord, Option<Blob>)>>>,
	pub(crate) cache_bytes: Arc<AtomicUsize>,
	scale_fn: ScaleDownFn,
}

impl OverviewCore {
	/// Build an overview core from a tile source.
	///
	/// - `level`: zoom level to build the overview from (defaults to source max zoom)
	/// - `scale_fn`: function that downscales an image by factor 2
	///
	/// The tile size is read from the source's TileJSON (defaults to 512 if not set).
	pub fn new(
		source: Box<dyn TileSource>,
		level: Option<u8>,
		scale_fn: ScaleDownFn,
		runtime: TilesRuntime,
	) -> Result<Self> {
		ensure!(source.metadata().traversal().is_any());

		let mut metadata = source.metadata().clone();
		let mut tilejson = source.tilejson().clone();

		let source_pyramid = metadata
			.tile_pyramid()
			.ok_or_else(|| anyhow::anyhow!("tile_pyramid not set on source"))?;
		let level_base = match level {
			Some(l) => l,
			None => source_pyramid
				.level_max()
				.ok_or_else(|| anyhow::anyhow!("source pyramid is empty"))?,
		};

		// The overview reads the source at `level_base` and composes everything
		// below it from what that produced, so a base level the source has no
		// tiles at leaves nothing to build from. Worth refusing rather than
		// accepting: the loop below is what extends the pyramid downward, it is
		// seeded from this level, and seeded empty it inserts nothing — leaving
		// an operation that answers `None` at every level and never says why.
		ensure!(
			!source_pyramid.level_ref(level_base).to_bbox().is_empty(),
			"the source has no tiles at level {level_base} to build an overview from; its levels are {:?}..={:?}",
			source_pyramid.level_min(),
			source_pyramid.level_max()
		);

		let mut new_pyramid = source_pyramid.as_ref().clone();
		let mut level_bbox = new_pyramid.level_ref(level_base).to_bbox();
		while !level_bbox.is_empty() && level_bbox.level() > 0 {
			level_bbox.level_down();
			new_pyramid.insert_bbox(&level_bbox)?;
		}
		metadata.set_tile_pyramid(new_pyramid);

		metadata.update_tilejson(&mut tilejson);

		// Guessing here is not harmless: the overview is rendered at this size, so
		// a 256 px source silently assumed to be 512 produces wrong tiles. The
		// fallback stays, because sources that never declared a size predate the
		// field being filled in (issue #247), but it says so.
		let tile_size = tilejson.tile_size.map_or_else(
			|| {
				log::warn!(
					"source declares no tile_size; assuming {DEFAULT_TILE_SIZE} px for the overview. If its tiles are a \
					 different size, the result will be wrong — set the size on the source, or insert `raster_tile_resize`."
				);
				DEFAULT_TILE_SIZE
			},
			|ts| u32::from(ts.size()),
		);
		let cache = Arc::new(DashMap::new());
		let cache_bytes = Arc::new(AtomicUsize::new(0));

		metadata.set_traversal(Traversal::new(
			TraversalOrder::DepthFirst,
			BLOCK_TILE_COUNT,
			BLOCK_TILE_COUNT,
		)?);

		Ok(Self {
			runtime,
			metadata,
			source,
			tilejson,
			level_base,
			tile_size,
			cache,
			cache_bytes,
			scale_fn,
		})
	}

	#[context("Failed to build images from cache for bbox {bbox:?}")]
	pub async fn build_images_from_cache(&self, bbox: TileBBox) -> Result<TileBBoxMap<Option<DynamicImage>>> {
		log::trace!("build_images_from_cache: {bbox:?}");

		let size = bbox.max_count().min(BLOCK_TILE_COUNT);

		ensure!(
			bbox.level() < self.level_base,
			"bbox level {} must be below the base level {}",
			bbox.level(),
			self.level_base
		);
		ensure!(
			bbox.width() <= size,
			"bbox width {} exceeds the block size {size}",
			bbox.width()
		);
		ensure!(
			bbox.height() <= size,
			"bbox height {} exceeds the block size {size}",
			bbox.height()
		);

		let bbox0 = bbox.rounded(size);
		assert_eq!(bbox0.width(), size);
		assert_eq!(bbox0.height(), size);

		let mut map: TileBBoxMap<Vec<(TileCoord, DynamicImage)>> = TileBBoxMap::new_default(bbox)?;

		let full_size = self.tile_size;
		let half_size = self.tile_size / 2;

		// get images from cache
		for q in &[0, 1, 2, 3] {
			let bbox1 = bbox0.leveled_up().quadrant(*q)?;

			if let Some((_, blobs1)) = self.cache.remove(&bbox1.min_tile()?) {
				let entry_bytes = estimate_entry_bytes(&blobs1);
				self.cache_bytes.fetch_sub(entry_bytes, Ordering::Relaxed);
				for (coord1, blob1) in blobs1 {
					if let Some(blob1) = blob1 {
						let image1 = versatiles_image::format::png::blob2image(&blob1)?;
						assert_eq!(image1.width(), half_size);
						assert_eq!(image1.height(), half_size);
						let coord0 = coord1.to_level_decreased()?;
						if bbox.includes_coord(&coord0) {
							map.get_mut(&coord0)?.push((coord1, image1));
						}
					}
				}
			}
		}

		let vec: Vec<(TileCoord, DynamicImage)> =
			futures::future::join_all(map.into_iter().filter(|(_, sub_entries)| !sub_entries.is_empty()).map(
				|(coord0, sub_entries)| {
					tokio::task::spawn_blocking(move || {
						let mut image_tmp = DynamicImage::new_rgba8(full_size, full_size);
						for (coord1, image1) in sub_entries {
							image_tmp
								.copy_from(&image1, (coord1.x % 2) * half_size, (coord1.y % 2) * half_size)
								.expect("half_size sub-image fits at quadrant offset");
						}
						image_tmp.into_optional().map(|image| (coord0, image))
					})
				},
			))
			.await
			.into_iter()
			.filter_map(|r| r.expect("spawn_blocking task did not panic"))
			.collect();

		TileBBoxMap::<Option<DynamicImage>>::from_iter(bbox, vec.into_iter())
	}

	/// Consume the container: scale each image down for the cache and wrap
	/// the original in a [`Tile`] — all in parallel, with zero image clones.
	#[context("Failed to scale and encode tiles for bbox {bbox:?}")]
	async fn scale_cache_and_encode(
		&self,
		container: TileBBoxMap<Option<DynamicImage>>,
		bbox: TileBBox,
	) -> Result<Vec<(TileCoord, Tile)>> {
		let container_bbox = *container.bbox();
		let format = *self.source.metadata().tile_format();
		let full_size = self.tile_size;
		let scale_fn = self.scale_fn.clone();
		let need_cache = container_bbox.level() > 0 && container_bbox.level() <= self.level_base;

		let results: Vec<_> = futures::future::join_all(container.into_iter().map(|(coord, image_opt)| {
			let scale_fn = scale_fn.clone();
			tokio::task::spawn_blocking(move || {
				let cached = if need_cache {
					image_opt.as_ref().map(|img| {
						assert_eq!(img.width(), full_size);
						assert_eq!(img.height(), full_size);
						let scaled = scale_fn(img).expect("scale_fn succeeds on valid overview image");
						versatiles_image::format::png::encode(&scaled, Some(0))
							.expect("png encode succeeds on scaled overview image")
					})
				} else {
					None
				};
				let tile = if bbox.includes_coord(&coord) {
					image_opt.map(|img| (coord, Tile::from_image(img, format).expect("source format is raster")))
				} else {
					None
				};
				((coord, cached), tile)
			})
		}))
		.await
		.into_iter()
		.collect::<Result<Vec<_>, _>>()?;

		let (cache_entries, tiles): (Vec<_>, Vec<_>) = results.into_iter().unzip();

		if need_cache {
			let mut key = container_bbox.min_tile()?;
			key.floor(BLOCK_TILE_COUNT);
			let entry_bytes = estimate_entry_bytes(&cache_entries);
			let total = self.cache_bytes.fetch_add(entry_bytes, Ordering::Relaxed) + entry_bytes;
			let gb = total as f64 / (1024.0 * 1024.0 * 1024.0);
			if gb > 2.0 {
				log::warn!("Overview staging area using {gb:.1} GB — consider reducing dataset size or base zoom level");
			}
			self.cache.insert(key, cache_entries);
		}

		Ok(tiles.into_iter().flatten().collect())
	}

	pub async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		if bbox.level() >= self.level_base {
			return self.source.tile_coord_stream(bbox).await;
		}

		let source_bbox_upsized = bbox.at_level(self.level_base);
		let source_bbox = self.metadata.intersection_bbox(&source_bbox_upsized);
		if source_bbox.is_empty() {
			return Ok(TileStream::empty());
		}

		let mut coords = std::collections::HashSet::new();
		let mut stream = self.source.tile_coord_stream(source_bbox).await?;
		while let Some((coord, _)) = stream.next().await {
			let c = coord.at_level(bbox.level());
			if bbox.includes_coord(&c) {
				coords.insert(c);
			}
		}

		let vec: Vec<(TileCoord, ())> = coords.into_iter().map(|c| (c, ())).collect();
		Ok(TileStream::from_vec(vec))
	}

	#[context("Failed to get stream for bbox: {:?}", bbox)]
	pub async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("overview::tile_stream {bbox:?}");

		if bbox.level() > self.level_base {
			return self.source.tile_stream(bbox).await;
		}

		let size = bbox.max_count().min(BLOCK_TILE_COUNT);
		let bbox0_raw = bbox.rounded(size);
		assert_eq!(bbox0_raw.width(), size);
		assert_eq!(bbox0_raw.height(), size);
		let bbox0 = self.metadata.intersection_bbox(&bbox0_raw);
		// Nothing of the source lies under this block, which is ordinary for any
		// raster that does not cover the whole world: below the base level a
		// request outside the data's footprint intersects to nothing. The answer
		// is an empty stream, the same one `tile_coord_stream` gives after its
		// own intersection above. Without this, `build_images_from_cache` is
		// handed an empty bbox — which passes all three of its `ensure!`s, since
		// a width of 0 is not too wide — and `TileBBox::round`, a documented
		// no-op on an empty bbox, leaves the width at 0 for its `assert_eq!` to
		// abort on. See issue #263.
		if bbox0.is_empty() {
			return Ok(TileStream::empty());
		}

		let container: TileBBoxMap<Option<DynamicImage>> = if bbox.level() == self.level_base {
			log::trace!("Fetching images from source for bbox {bbox:?}");
			TileBBoxMap::<Option<DynamicImage>>::from_stream(
				bbox,
				self
					.source
					.tile_stream(bbox)
					.await?
					.map_parallel_try(|_coord, tile| tile.into_image())
					.record_errors(&self.runtime, "overview"),
			)
			.await?
		} else {
			log::trace!("Building images from cache for bbox {bbox:?}");
			self.build_images_from_cache(bbox0).await?
		};

		log::trace!("Scaling, caching, and encoding tiles for bbox {bbox:?}");
		let vec = self.scale_cache_and_encode(container, bbox).await?;

		Ok(TileStream::from_vec(vec))
	}
}

#[expect(clippy::cast_possible_truncation, reason = "a blob length, already a usize")]
pub(crate) fn estimate_entry_bytes(entries: &[(TileCoord, Option<Blob>)]) -> usize {
	entries
		.iter()
		.map(|(_, blob)| blob.as_ref().map_or(16, |b| b.len() as usize))
		.sum()
}

impl std::fmt::Debug for OverviewCore {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("OverviewCore")
			.field("level_base", &self.level_base)
			.field("tile_size", &self.tile_size)
			.field("metadata", &self.metadata)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use anyhow::Result;
	use versatiles_core::{GeoBBox, TileFormat, TilePyramid};
	use versatiles_image::traits::DynamicImageTraitOperation;

	use super::*;
	use crate::helpers::dummy_image_source::DummyImageSource;

	fn make_core(level_base: u8) -> Result<OverviewCore> {
		let pyramid = TilePyramid::from_geo_bbox(
			level_base,
			level_base,
			&GeoBBox::new(2.224, 48.815, 2.47, 48.903).unwrap(),
		)?;
		let source =
			Box::new(DummyImageSource::from_color(&[200u8, 100, 50], 256, TileFormat::PNG, Some(pyramid)).unwrap());
		let scale_fn: ScaleDownFn = Arc::new(|img| img.scaled_down(2));
		OverviewCore::new(source, Some(level_base), scale_fn, TilesRuntime::new_silent())
	}

	#[tokio::test]
	async fn tile_coord_stream_at_base_level_delegates_to_source() -> Result<()> {
		let core = make_core(5)?;
		let bbox = TileBBox::new_full(5)?;
		let coords = core.tile_coord_stream(bbox).await?.to_vec().await;
		assert!(!coords.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn tile_stream_above_base_level_delegates_to_source() -> Result<()> {
		let core = make_core(4)?;
		let bbox = TileBBox::new_full(5)?;
		// Source only has tiles at level 4; requesting level 5 passes through to source
		let tiles = core.tile_stream(bbox).await?.to_vec().await;
		// source has no tiles at level 5
		let _ = tiles;
		Ok(())
	}

	#[tokio::test]
	async fn build_images_from_cache_rejects_level_gte_base() -> Result<()> {
		let core = make_core(5)?;
		let bbox = TileBBox::new_full(5)?;
		let result = core.build_images_from_cache(bbox).await;
		assert!(result.is_err());
		Ok(())
	}

	/// A tile below the base level, outside the data's footprint, is `None`.
	///
	/// This needs no misconfiguration: `make_core` is a correctly built overview
	/// over Paris, and any raster that is not the whole world has the same
	/// shape. Asking for a level-4 tile off the coast of Africa used to abort
	/// the process on an `assert_eq!` deep in `build_images_from_cache`, because
	/// the intersection with the source's pyramid is empty there and an empty
	/// bbox satisfies every guard on the way in. See issue #263.
	#[tokio::test]
	async fn tile_stream_outside_the_data_below_the_base_level_is_empty() -> Result<()> {
		// Base level 8, so the level below spans 128 tiles and a 16-wide block can
		// miss the data. At level 5 it could not: the whole level is one block
		// wide, so rounding always covers Paris again and nothing is ever empty.
		let core = make_core(8)?;

		// Paris sits near (64, 41) on level 7; this block covers (0..15, 0..15).
		let far_away = TileBBox::from_min_and_max(7, 0, 0, 0, 0)?;
		let tiles = core.tile_stream(far_away).await?.to_vec().await;
		assert!(
			tiles.is_empty(),
			"expected no tiles away from the data, got {}",
			tiles.len()
		);

		// The same request through the sibling method, which already guarded it.
		let coords = core.tile_coord_stream(far_away).await?.to_vec().await;
		assert!(coords.is_empty());

		Ok(())
	}

	/// A base level the source has no tiles at is refused when the operation is
	/// built, not discovered as silence at every zoom.
	///
	/// The pyramid is extended downward from this level, so seeding it from an
	/// empty one inserts nothing and leaves an overview that answers `None`
	/// everywhere. Before issue #263 this configuration panicked instead; the
	/// guard in `tile_stream` turns that into `None`, and this turns it into an
	/// error that names the cause.
	#[tokio::test]
	async fn a_base_level_the_source_has_no_tiles_at_is_refused() -> Result<()> {
		let pyramid = TilePyramid::from_geo_bbox(8, 8, &GeoBBox::new(2.224, 48.815, 2.47, 48.903).unwrap())?;
		let source =
			Box::new(DummyImageSource::from_color(&[200u8, 100, 50], 256, TileFormat::PNG, Some(pyramid)).unwrap());
		let scale_fn: ScaleDownFn = Arc::new(|img| img.scaled_down(2));

		let error = OverviewCore::new(source, Some(3), scale_fn, TilesRuntime::new_silent())
			.expect_err("level 3 holds no tiles to build from");

		assert!(
			error.to_string().contains("no tiles at level 3"),
			"expected the level to be named, got: {error}"
		);

		Ok(())
	}

	#[tokio::test]
	async fn tile_coord_stream_below_base_level_returns_coords() -> Result<()> {
		let core = make_core(5)?;
		let bbox = TileBBox::new_full(4)?;
		let coords = core.tile_coord_stream(bbox).await?.to_vec().await;
		assert!(!coords.is_empty());
		Ok(())
	}
}
