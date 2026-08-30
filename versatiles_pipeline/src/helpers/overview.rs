use std::sync::{
	Arc,
	atomic::{AtomicU64, Ordering},
};

use anyhow::{Result, anyhow, ensure};
use futures::FutureExt;
use imageproc::image::{DynamicImage, GenericImage};
use versatiles_container::{
	Tile, TileSource, TileSourceMetadata, TileStreamErrorExt, TilesRuntime, Traversal, TraversalOrder,
};
use versatiles_core::{Blob, TileBBox, TileBBoxMap, TileCoord, TileJSON, TileStream};
use versatiles_derive::context;
use versatiles_image::traits::DynamicImageTraitInfo;

use super::overview_cache::BlockCache;

static BLOCK_TILE_COUNT: u32 = 16;

/// Tile size assumed when a source declares none.
const DEFAULT_TILE_SIZE: u32 = 512;

/// Scaling function type used to downscale tiles by a factor of 2.
pub type ScaleDownFn = Arc<dyn Fn(&DynamicImage) -> Result<DynamicImage> + Send + Sync>;

/// Source tiles one interactive request may read before it gives up.
///
/// Reaching this means the request asked for a tile far above a dense source:
/// a tile at zoom `z` costs `256 * 4^(level - z)` source reads from cold, so
/// four levels above a fully covered base is already the whole base level. Such
/// a request cannot finish inside a tile server's request timeout anyway, and
/// failing immediately with the numbers beats spending minutes on a result
/// nobody is still waiting for.
const DEFAULT_REQUEST_BUDGET: u64 = 65_536;

/// Overrides [`DEFAULT_REQUEST_BUDGET`]; `0` removes the limit.
const REQUEST_BUDGET_ENV: &str = "VERSATILES_OVERVIEW_REQUEST_TILES";

/// What one request has left to spend on source tiles.
///
/// Only [`OverviewCore::tile`] carries one. Bulk streaming does not: a
/// conversion asks for the whole pyramid on purpose, and the single call that
/// builds zoom 0 legitimately reads everything underneath it.
#[derive(Debug)]
pub struct ReadBudget {
	remaining: AtomicU64,
	limit: u64,
}

impl ReadBudget {
	/// A budget of `limit` source tiles.
	#[must_use]
	pub fn new(limit: u64) -> Self {
		Self {
			remaining: AtomicU64::new(limit),
			limit,
		}
	}

	/// Spend `tiles` of the budget, or explain why the request stops here.
	fn take(&self, tiles: u64, bbox: TileBBox) -> Result<()> {
		let left = self
			.remaining
			.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| left.checked_sub(tiles));

		ensure!(
			left.is_ok(),
			"this tile needs more than {} source tiles to build, and {bbox:?} is where the budget ran out. Building a \
			 low zoom level on demand costs 256 x 4^(levels below the base) source tiles, so over a dense source it is \
			 work for a conversion, not for a request someone is waiting on: convert once and serve the result. Raise \
			 or remove the ceiling with {REQUEST_BUDGET_ENV} (tiles, 0 disables it).",
			self.limit
		);
		Ok(())
	}
}

/// The ceiling [`REQUEST_BUDGET_ENV`] asks for, as a number of source tiles.
///
/// Takes the raw value rather than reading the environment itself, so the
/// parsing can be tested without a process-wide variable that every other test
/// would also see. `None` means the ceiling is disabled.
fn parse_request_budget(raw: Option<&str>) -> Option<u64> {
	match raw {
		None => Some(DEFAULT_REQUEST_BUDGET),
		Some(raw) => match raw.trim().parse::<u64>() {
			Ok(0) => None,
			Ok(limit) => Some(limit),
			Err(_) => {
				log::warn!(
					"{REQUEST_BUDGET_ENV} is \"{raw}\", which is not a number of tiles; using {DEFAULT_REQUEST_BUDGET}"
				);
				Some(DEFAULT_REQUEST_BUDGET)
			}
		},
	}
}

/// One block of tiles, each already downscaled by half and PNG-encoded.
///
/// This is how a zoom level is remembered: four of these placed side by side
/// are exactly one block of full-size tiles at the level below.
pub type ScaledEntries = Vec<(TileCoord, Option<Blob>)>;

/// A [`ScaledEntries`] shared between the cache and everything reading it.
pub type ScaledBlock = Arc<ScaledEntries>;

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
	/// Scaled blocks, keyed by the northwest tile of the block they cover.
	///
	/// This is a memo table, not a hand-off buffer: reading a block leaves it
	/// available, and a block that is not there is computed rather than
	/// reported as missing. That is what makes the operation answer the same
	/// thing however the requests arrive — a tile server asking for one tile
	/// out of nowhere gets the same bytes as a converter walking the pyramid.
	pub cache: BlockCache,
	/// Source tiles a single request may read, or `None` when the ceiling is
	/// switched off. Read from the environment once, when the operation is
	/// built, rather than on every request.
	request_budget: Option<u64>,
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

		let cache = BlockCache::new(level_base);
		let request_budget = parse_request_budget(std::env::var(REQUEST_BUDGET_ENV).ok().as_deref());

		// A preference, not a requirement. Every zoom level here is built from
		// the one above it, so a depth-first walk produces each block just
		// before the level below consumes it and nothing is ever built twice —
		// but any other order still gives the same tiles, at the cost of
		// rebuilding. Declaring depth-first as a *requirement* is what used to
		// force PMTiles to choose between staying clustered and writing the
		// tile data twice, for an ordering this operation can live without.
		metadata.set_preferred_traversal(Traversal::new(
			TraversalOrder::DepthFirst,
			BLOCK_TILE_COUNT,
			BLOCK_TILE_COUNT,
		)?);
		metadata.set_traversal(Traversal::ANY);

		Ok(Self {
			runtime,
			metadata,
			source,
			tilejson,
			level_base,
			tile_size,
			cache,
			request_budget,
			scale_fn,
		})
	}

	/// The block a cache key stands for: `BLOCK_TILE_COUNT` tiles square, or
	/// the whole level when the level is smaller than one block.
	fn block_bbox(key: TileCoord) -> Result<TileBBox> {
		let size = BLOCK_TILE_COUNT.min(1u32 << key.level);
		TileBBox::from_min_and_size(key.level, key.x, key.y, size, size)
	}

	/// The blocks covering `bbox`, in the grid the cache is keyed on.
	fn blocks_covering(bbox: TileBBox) -> Vec<TileBBox> {
		let size = BLOCK_TILE_COUNT.min(bbox.max_count());
		bbox.rounded(size).iter_grid(size).collect()
	}

	/// Full-size images for `bbox`: read from the source at the base level,
	/// composed from the level above it anywhere below that.
	///
	/// Boxed because this and [`OverviewCore::scaled_block`] call each other —
	/// a plain pair of `async fn`s would have infinitely sized futures.
	pub fn full_images(
		&self,
		bbox: TileBBox,
		budget: Option<Arc<ReadBudget>>,
	) -> futures::future::BoxFuture<'_, Result<TileBBoxMap<Option<DynamicImage>>>> {
		async move {
			if bbox.level() == self.level_base {
				log::trace!("overview: reading images from the source for {bbox:?}");
				// The only place source tiles are read, so the only place a
				// budget is spent. Charged against what the source can actually
				// have here rather than the size of the request, so a sparse
				// pyramid is not billed for the emptiness around its data.
				if let Some(budget) = &budget {
					budget.take(self.metadata.intersection_bbox(&bbox).count_tiles(), bbox)?;
				}
				return TileBBoxMap::<Option<DynamicImage>>::from_stream(
					bbox,
					self
						.source
						.tile_stream(bbox)
						.await?
						.map_parallel_try(|_coord, tile| tile.into_image())
						.record_errors(&self.runtime, "overview"),
				)
				.await;
			}
			self.compose_from_children(bbox, budget).await
		}
		.boxed()
	}

	/// The scaled block at `key`, computing it if neither tier has it.
	///
	/// The recursion underneath cannot deadlock on the per-key guard the cache
	/// holds while computing: a block is built only from blocks one level
	/// *closer to the base*, so the keys a computation waits on are strictly
	/// deeper than its own and no cycle can form.
	fn scaled_block(
		&self,
		key: TileCoord,
		budget: Option<Arc<ReadBudget>>,
	) -> futures::future::BoxFuture<'_, Result<ScaledBlock>> {
		async move {
			// A known rough edge, kept on purpose.
			//
			// `get_or_compute` coalesces concurrent callers for one key into a
			// single computation, and the budget travels *inside* that
			// computation. So the caller that wins the race spends its budget on
			// behalf of everyone waiting, and they all receive its result —
			// including its error. Two clients asking for neighbouring low-zoom
			// tiles at the same moment can therefore see one refused for the
			// other's spending, with its own budget untouched.
			//
			// Both alternatives cost more than the defect. Billing each waiter
			// after the fact would mean the ceiling could only notice a runaway
			// build once it had finished, which is most of what it is for;
			// keying the coalescing by budget would make concurrent requests for
			// the same region each rebuild it, which is the sharing that makes
			// them cheap. The failure is transient — the cache does not keep the
			// error, so a retry recomputes — and it only reaches requests for a
			// region that was already over the ceiling.
			self
				.cache
				.get_or_compute(key, self.compute_scaled_block(key, budget))
				.await
				// The cache hands back a shared error, which `anyhow` cannot take
				// ownership of; keep the whole chain as the message.
				.map_err(|e: Arc<anyhow::Error>| anyhow!("{e:#}"))
		}
		.boxed()
	}

	#[context("Failed to compute the scaled block at {key:?}")]
	async fn compute_scaled_block(&self, key: TileCoord, budget: Option<Arc<ReadBudget>>) -> Result<ScaledBlock> {
		let bbox = Self::block_bbox(key)?;
		log::trace!("overview: computing scaled block {bbox:?}");
		let container = self.full_images(bbox, budget).await?;
		let (entries, _) = self.scale_and_emit(container, None, true).await?;
		Ok(Arc::new(entries))
	}

	/// Compose full-size images for `bbox` out of the four blocks above it.
	///
	/// Each source block holds tiles already downscaled by half, so a tile here
	/// is its four children placed in quadrants — no scaling happens on this
	/// path, only assembly.
	#[context("Failed to compose images for bbox {bbox:?}")]
	pub async fn compose_from_children(
		&self,
		bbox: TileBBox,
		budget: Option<Arc<ReadBudget>>,
	) -> Result<TileBBoxMap<Option<DynamicImage>>> {
		log::trace!("compose_from_children: {bbox:?}");

		ensure!(
			bbox.level() < self.level_base,
			"bbox level {} must be below the base level {}",
			bbox.level(),
			self.level_base
		);

		let block = bbox.rounded(BLOCK_TILE_COUNT.min(bbox.max_count()));
		ensure!(
			block.width() <= BLOCK_TILE_COUNT && block.height() <= BLOCK_TILE_COUNT,
			"bbox {bbox:?} spans more than one block"
		);

		let mut map: TileBBoxMap<Vec<(TileCoord, DynamicImage)>> = TileBBoxMap::new_default(bbox)?;

		let full_size = self.tile_size;
		let half_size = self.tile_size / 2;

		// Collected rather than iterated lazily: `iter_grid` hands back a boxed
		// iterator that is not `Send`, and this loop awaits inside.
		let children: Vec<TileBBox> = block.leveled_up().iter_grid(BLOCK_TILE_COUNT).collect();

		for child in children {
			let key = child.min_tile()?;

			// The pyramid check is a shortcut past *computing* a block, not a
			// reason to ignore one already held: nothing of the source lies
			// under this child, which is ordinary for any raster that does not
			// cover the whole world, and building it would walk all the way
			// down to the base level only to find nothing there.
			let block = match self.cache.consume(key).await {
				Some(block) => block,
				None if self.metadata.intersection_bbox(&child).is_empty() => continue,
				None => self.scaled_block(key, budget.clone()).await?,
			};

			for (coord1, blob1) in block.iter() {
				let Some(blob1) = blob1 else { continue };
				let coord0 = coord1.to_level_decreased()?;
				if !bbox.includes_coord(&coord0) {
					continue;
				}
				let image1 = versatiles_image::format::png::blob2image(blob1)?;
				ensure!(
					image1.width() == half_size && image1.height() == half_size,
					"cached tile {coord1:?} is {}x{} px, expected {half_size} px square",
					image1.width(),
					image1.height()
				);
				map.get_mut(&coord0)?.push((*coord1, image1));
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

	/// Consume the container: scale each image down for the cache and wrap the
	/// ones inside `emit` in a [`Tile`] — all in parallel, with zero image
	/// clones. Skipping either half costs nothing, so a caller that only wants
	/// tiles does not pay for scaling, and vice versa.
	#[context("Failed to scale and encode tiles for {emit:?}")]
	async fn scale_and_emit(
		&self,
		container: TileBBoxMap<Option<DynamicImage>>,
		emit: Option<TileBBox>,
		scale: bool,
	) -> Result<(ScaledEntries, Vec<(TileCoord, Tile)>)> {
		let format = *self.source.metadata().tile_format();
		let full_size = self.tile_size;
		let scale_fn = self.scale_fn.clone();

		let results: Vec<_> = futures::future::join_all(container.into_iter().map(|(coord, image_opt)| {
			let scale_fn = scale_fn.clone();
			tokio::task::spawn_blocking(move || {
				let cached = if scale {
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
				let tile = if emit.is_some_and(|bbox| bbox.includes_coord(&coord)) {
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

		Ok((cache_entries, tiles.into_iter().flatten().collect()))
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

	/// One tile, for a caller waiting on it.
	///
	/// The difference from [`tile_stream`](Self::tile_stream) is the budget.
	/// Bulk streaming is a conversion asking for the whole pyramid on purpose,
	/// and the one call that builds zoom 0 legitimately reads everything
	/// underneath it; a single tile is someone waiting, so there is a ceiling
	/// on how much of the pyramid it may build to answer them.
	#[context("Failed to get tile {coord:?}")]
	pub async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		let budget = self.request_budget.map(|limit| Arc::new(ReadBudget::new(limit)));
		self.tile_within(coord, budget).await
	}

	/// Replace the per-request ceiling.
	///
	/// Exists so a test can prove that a `TileSource::tile` implementation
	/// really routes here — the ceiling is the only observable difference
	/// between doing so and letting the trait's default take the first item of
	/// a one-tile stream, and the alternative way to set it is an environment
	/// variable that every other test in the process would also see.
	#[cfg(test)]
	pub(crate) fn set_request_budget(&mut self, limit: Option<u64>) {
		self.request_budget = limit;
	}

	/// [`tile`](Self::tile) with the budget given rather than read from the
	/// environment, so a test can reach the ceiling without a pyramid big
	/// enough to cost what the ceiling is worth.
	pub(crate) async fn tile_within(&self, coord: &TileCoord, budget: Option<Arc<ReadBudget>>) -> Result<Option<Tile>> {
		let mut stream = self.tile_stream_within(coord.to_tile_bbox(), budget).await?;
		Ok(stream.next().await.map(|(_, tile)| tile))
	}

	#[context("Failed to get stream for bbox: {:?}", bbox)]
	pub async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		self.tile_stream_within(bbox, None).await
	}

	async fn tile_stream_within(
		&self,
		bbox: TileBBox,
		budget: Option<Arc<ReadBudget>>,
	) -> Result<TileStream<'static, Tile>> {
		log::trace!("overview::tile_stream {bbox:?}");

		if bbox.level() > self.level_base {
			return self.source.tile_stream(bbox).await;
		}

		let mut tiles: Vec<(TileCoord, Tile)> = Vec::new();

		for block in Self::blocks_covering(bbox) {
			// What the source can possibly have in this block. An empty
			// intersection is ordinary for any raster that does not cover the
			// whole world: below the base level a request outside the data's
			// footprint intersects to nothing. The answer is no tiles, the same
			// one `tile_coord_stream` gives after its own intersection above.
			// See issue #263.
			let present = self.metadata.intersection_bbox(&block);
			if present.is_empty() {
				continue;
			}

			// Only the part of the block the caller asked for. Computing the
			// whole block regardless would mean 256 tiles of work to answer a
			// tile server asking for one.
			let mut wanted = block;
			wanted.intersect_bbox(&bbox)?;
			if wanted.is_empty() {
				continue;
			}

			// Caching a partial block would be worse than not caching it: the
			// entry would be indistinguishable from a complete one and every
			// level built on top of it would silently be missing tiles. So the
			// block is only remembered once what was computed covers everything
			// the source has here — which is exactly what a traversal asks for,
			// since it clips its blocks to the pyramid too. Level 0 has nothing
			// above it to be composed into and is never worth keeping.
			let worth_caching = block.level() > 0 && wanted.includes_bbox(&present);

			let container = self.full_images(wanted, budget.clone()).await?;
			let (entries, block_tiles) = self.scale_and_emit(container, Some(bbox), worth_caching).await?;

			if worth_caching {
				self.cache.produce(block.min_tile()?, Arc::new(entries)).await;
			}
			tiles.extend(block_tiles);
		}

		Ok(TileStream::from_vec(tiles))
	}
}

#[expect(clippy::cast_possible_truncation, reason = "a blob length, already a usize")]
pub(crate) fn estimate_entry_bytes(entries: &[(TileCoord, Option<Blob>)]) -> usize {
	entries
		.iter()
		.map(|(_, blob)| blob.as_ref().map_or(16, |b| b.len() as usize))
		.sum()
}

impl Drop for OverviewCore {
	/// Report what the cache had to do, once, when the operation goes away.
	///
	/// `blocks_computed` is zero for a depth-first pass: every block is
	/// produced just before the level below asks for it, so nothing is ever
	/// built a second time. A number above zero is the cost of arriving in some
	/// other order — which is legitimate, but worth being able to see.
	fn drop(&mut self) {
		let (computed, peak_pending) = self.cache.stats();
		if computed > 0 || peak_pending > 0 {
			log::debug!(
				"overview: {computed} block(s) built on demand, {} MiB peak in blocks awaiting their consumer",
				peak_pending >> 20
			);
		}
	}
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
	async fn compose_from_children_rejects_level_gte_base() -> Result<()> {
		let core = make_core(5)?;
		let bbox = TileBBox::new_full(5)?;
		let result = core.compose_from_children(bbox, None).await;
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

	// -----------------------------------------------------------------
	// The ceiling on what one request may build.
	// -----------------------------------------------------------------

	/// A source covering enough ground that one block holds many tiles.
	///
	/// `make_core` covers a single city, which is one tile at zoom 8 — too
	/// little to spend a budget on.
	fn make_wide_core(level_base: u8) -> Result<OverviewCore> {
		let pyramid =
			TilePyramid::from_geo_bbox(level_base, level_base, &GeoBBox::new(-30.0, 20.0, 30.0, 60.0).unwrap())?;
		let source =
			Box::new(DummyImageSource::from_color(&[200u8, 100, 50], 256, TileFormat::PNG, Some(pyramid)).unwrap());
		let scale_fn: ScaleDownFn = Arc::new(|img| img.scaled_down(2));
		OverviewCore::new(source, Some(level_base), scale_fn, TilesRuntime::new_silent())
	}

	#[tokio::test]
	async fn a_request_that_would_build_too_much_is_refused() -> Result<()> {
		let core = make_wide_core(8)?;
		let coord = core
			.metadata
			.tile_pyramid()
			.unwrap()
			.level_ref(4)
			.to_bbox()
			.min_tile()?;

		let error = core
			.tile_within(&coord, Some(Arc::new(ReadBudget::new(1))))
			.await
			.expect_err("one tile of budget cannot pay for a whole block");
		let error = format!("{error:#}");

		assert!(
			error.contains("source tiles to build"),
			"should say what ran out: {error}"
		);
		assert!(
			error.contains(REQUEST_BUDGET_ENV),
			"should name the way to raise it: {error}"
		);
		Ok(())
	}

	/// A warm request is not refused, however expensive it would have been cold.
	///
	/// This is why the ceiling counts tiles actually read rather than
	/// predicting the cost from the pyramid: the prediction cannot see the
	/// cache, so it would refuse exactly the requests that are already cheap —
	/// the neighbours a tile server asks for right after the first one.
	#[tokio::test]
	async fn a_warm_request_is_not_refused() -> Result<()> {
		let core = make_core(8)?;
		let coord = core
			.metadata
			.tile_pyramid()
			.unwrap()
			.level_ref(4)
			.to_bbox()
			.min_tile()?;

		// Cold, with room to spare; then again with a budget of nothing.
		core
			.tile_within(&coord, Some(Arc::new(ReadBudget::new(u64::MAX))))
			.await?;

		let warm = core.tile_within(&coord, Some(Arc::new(ReadBudget::new(0)))).await?;
		assert!(warm.is_some(), "a request that reads nothing should not be refused");
		Ok(())
	}

	/// Bulk streaming has no ceiling.
	///
	/// A conversion asks for the whole pyramid on purpose, and the single call
	/// that builds zoom 0 legitimately reads everything underneath it.
	#[tokio::test]
	async fn bulk_streaming_is_not_budgeted() -> Result<()> {
		let core = make_core(8)?;
		let coord = core
			.metadata
			.tile_pyramid()
			.unwrap()
			.level_ref(4)
			.to_bbox()
			.min_tile()?;

		assert!(
			core
				.tile_within(&coord, Some(Arc::new(ReadBudget::new(0))))
				.await
				.is_err(),
			"a budget of nothing should refuse a request that has to read"
		);

		let tiles = core.tile_stream(coord.to_tile_bbox()).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "the same tile, streamed, should not be refused");
		Ok(())
	}

	#[test]
	fn the_request_ceiling_is_read_in_tiles() {
		assert_eq!(parse_request_budget(None), Some(DEFAULT_REQUEST_BUDGET));
		assert_eq!(parse_request_budget(Some("100")), Some(100));
		assert_eq!(parse_request_budget(Some(" 100 ")), Some(100));
		assert_eq!(parse_request_budget(Some("0")), None, "0 removes the ceiling");
		assert_eq!(parse_request_budget(Some("many")), Some(DEFAULT_REQUEST_BUDGET));
		assert_eq!(parse_request_budget(Some("-1")), Some(DEFAULT_REQUEST_BUDGET));
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
