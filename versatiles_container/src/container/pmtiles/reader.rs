//! Read tiles and metadata from a `PMTiles` (v3) container.
//!
//! The `PMTilesReader` parses the `PMTiles` v3 header and directory structure, reads the
//! embedded `TileJSON` metadata, and fetches tile blobs by translating XYZ tile coordinates
//! into **Hilbert indices**. It supports internal compression used by `PMTiles` for
//! metadata/directories (e.g., gzip) as well as the **transport compression** of the tiles
//! themselves (e.g., gzip for MVT tiles) as declared in the header.
//!
//! ## What it extracts
//! - `header`: parsed [`HeaderV3`] with offsets and compression flags
//! - `tilejson`: parsed `TileJSON` (from `metadata` range), merged into [`TileJSON`]
//! - `parameters`: [`TileSourceMetadata`] with `tile_format`, `tile_compression`, and a
//!   computed **tile pyramid** inferred from the directory tree
//!
//! ## Requirements
//! - Use an **absolute** filesystem path when opening via [`PMTilesReader::open`].
//! - The container must be a valid `PMTiles` v3 file with readable header, directories, and data.
//!
//! ## Usage
//! ```rust,no_run
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Open PMTiles via absolute path
//!     let runtime = TilesRuntime::default();
//!     let path = Path::new("/absolute/path/to/berlin.pmtiles");
//!     let mut reader = PMTilesReader::open(path, runtime).await?;
//!
//!     // Inspect metadata
//!     let tj = reader.tilejson();
//!     println!("format={:?} compression={:?}", reader.metadata().tile_format(), reader.metadata().tile_compression());
//!
//!     // Fetch a tile
//!     let coord = TileCoord::new(14, 8800, 5370)?;
//!     if let Some(mut tile) = reader.tile(&coord).await? {
//!         let _blob = tile.as_blob(reader.metadata().tile_compression())?;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Errors
//! Returns errors when the path is not absolute, the file cannot be read, the
//! `PMTiles` header/directories cannot be parsed or decompressed, or a requested tile is missing.

use std::{fmt::Debug, mem::size_of, path::Path, sync::Arc};

use anyhow::{Context, Result};
use async_trait::async_trait;
use moka::future::Cache;
#[cfg(feature = "cli")]
use versatiles_core::utils::PrettyPrint;
use versatiles_core::{
	Blob, ByteRange, GeoBBox, TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream,
	compression::decompress,
	io::{DataReader, DataReaderFile},
	utils::HilbertIndex,
};
use versatiles_derive::context;

use super::types::{EntriesV3, HeaderV3};
use crate::{
	SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata, TilesReader, TilesRuntime, Traversal,
	TraversalOrder, TraversalSize, container::tile_chunking::Chunks, progress::ProgressHandle,
};

/// How many directory levels a lookup or a pyramid walk may descend.
///
/// Leaf directories are followed by reading a byte range out of the leaf blob,
/// and nothing stops an entry from naming the bytes it was itself parsed from.
/// The pyramid walk is recursive, so directory depth is stack depth: a
/// self-referential entry recurses until the stack is gone, and a stack overflow
/// aborts the process instead of unwinding — neither `catch_unwind` nor the
/// server's `CatchPanicLayer` can turn that back into an error. A 132-byte file
/// is enough to trigger it.
///
/// `PMTiles` v3 stores a root directory and one level of leaves, so two is what
/// the format produces in practice; three leaves room to spare and is the bound
/// the tile-lookup path has always applied.
const MAX_DIRECTORY_DEPTH: usize = 3;

/// The error for a tile whose directory entry is nested deeper than
/// [`MAX_DIRECTORY_DEPTH`]. The tile is in the file and cannot be reached, which
/// is not the same as absent: the two lookups used to disagree, one calling it
/// "not found", the other dropping it as a gap.
fn too_deep() -> anyhow::Error {
	anyhow::anyhow!(
		"PMTiles leaf directories nest more than {MAX_DIRECTORY_DEPTH} levels deep; the file is malformed or a leaf \
		 directory points at itself"
	)
}

/// Ceiling on how many tiles the directory walk may expand when the header does
/// not say.
///
/// `addressed_tiles_count` is the header's own count of addressed tiles, and it
/// is the natural bound on the walk — but writers are allowed to leave it `0`,
/// and a crafted header can name any number it likes. This caps both cases. The
/// largest real archives — a planet at zoom 15 — address on the order of 1.4
/// billion tiles, so 2^32 keeps three times that in reserve while still refusing
/// a `run_length` that exists only to make the reader count to 2^32.
const MAX_UNDECLARED_ADDRESSED_TILES: u64 = 1 << 32;

/// The directory walk passed its limit; [`calc_tile_pyramid`] says which.
#[derive(Debug)]
struct TooManyTiles;

impl std::fmt::Display for TooManyTiles {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("PMTiles directories address more tiles than allowed")
	}
}

impl std::error::Error for TooManyTiles {}

/// Reader for `PMTiles` v3 containers.
///
/// Parses the header and directory blobs, merges embedded `TileJSON`, computes a
/// tile pyramid by traversing directory entries, and exposes tiles via
/// the [`TileSource`] interface.
#[derive(Debug)]
pub struct PMTilesReader {
	/// Underlying byte source used to read header, directories, and tile data.
	pub data_reader: Arc<DataReader>,
	/// Parsed `PMTiles` v3 header with byte ranges, counts, and compression flags.
	pub header: HeaderV3,
	/// Compression algorithm used for internal metadata/directories (e.g., gzip).
	pub internal_compression: TileCompression,
	/// Raw (compressed) concatenated blob of all leaf directories.
	pub leaves_bytes: Arc<Blob>,
	/// Decompression cache mapping leaf directory byte ranges to parsed entries.
	/// Concurrent loads of the same range dedup automatically; distinct ranges
	/// decompress in parallel.
	pub leaves_cache: Cache<ByteRange, Arc<EntriesV3>>,
	/// Merged `TileJSON` metadata extracted from the `PMTiles` `metadata` range.
	pub tilejson: TileJSON,
	/// Runtime parameters (tile format, compression, tile pyramid) advertised by this reader.
	pub metadata: TileSourceMetadata,
	/// Uncompressed root directory blob.
	pub root_bytes_uncompressed: Blob,
	/// Parsed entries of the root directory (shared across queries).
	pub root_entries: Arc<EntriesV3>,
	/// Where a failed chunk read is reported, so the caller's abort-on-error
	/// policy decides whether the run ends. See `Chunks::stream`.
	pub runtime: TilesRuntime,
}

impl PMTilesReader {
	/// Open a `PMTiles` container from an **absolute** filesystem path.
	///
	/// Validates and opens a `DataReaderFile`, then delegates to [`PMTilesReader::open_data`].
	///
	/// # Errors
	/// Returns an error if the file cannot be opened.
	#[context("opening PMTiles at '{}'", path.display())]
	pub async fn open(path: &Path, runtime: TilesRuntime) -> Result<PMTilesReader> {
		PMTilesReader::open_data(DataReaderFile::open(path)?, runtime).await
	}

	/// Open a `PMTiles` container from an existing [`DataReader`].
	///
	/// Reads the v3 header, decompresses and parses the metadata (`TileJSON`) and
	/// root directory, prepares leaf directory bytes, computes the tile pyramid, and
	/// initializes caches for fast lookups.
	///
	/// # Errors
	/// Returns an error if reading or decompression fails, or if the header/dirs are invalid.
	#[context("opening PMTiles from reader")]
	pub async fn open_data(data_reader: DataReader, runtime: TilesRuntime) -> Result<PMTilesReader>
	where
		Self: Sized,
	{
		log::debug!("Opening PMTilesReader for {}", data_reader.name());

		let header = HeaderV3::deserialize(&data_reader.read_range(&ByteRange::new(0, HeaderV3::len())).await?)?;
		log::trace!("Header: {header:?}");

		let internal_compression = header.internal_compression.as_value()?;
		log::trace!("Internal compression: {internal_compression:?}");

		// Header is read first; the next three reads (metadata, root_dir, leaf_dirs)
		// hit disjoint, known ranges and can fan out concurrently. For HTTP-backed
		// readers this saves ~2 RTTs on every open.
		// Files with all entries in the root dir have leaf_dirs.length == 0; skip
		// that read so we don't issue a no-op HTTP request.
		let metadata_fut = data_reader.read_range(&header.metadata);
		let root_dir_fut = data_reader.read_range(&header.root_dir);
		// The leaf directories are most of what opening a large remote file
		// waits for, so they report how much has arrived (#304).
		let leaf_dirs_fut = async {
			if header.leaf_dirs.length == 0 {
				Ok::<Blob, anyhow::Error>(Blob::default())
			} else {
				let progress = runtime.create_progress("reading the tile index", header.leaf_dirs.length);
				let leaves = data_reader
					.read_range_with_progress(&header.leaf_dirs, &|bytes| progress.inc(bytes))
					.await;
				progress.finish();
				leaves
			}
		};
		let (meta, root_bytes, leaves_bytes) =
			futures::future::try_join3(metadata_fut, root_dir_fut, leaf_dirs_fut).await?;

		let meta = decompress(meta, &internal_compression)?;
		let mut tilejson = TileJSON::try_from_blob_or_default(&meta);
		log::trace!("TileJSON: {tilejson:?}");

		log::trace!("Root directory bytes length: {}", root_bytes.len());
		let root_bytes_uncompressed = decompress(root_bytes, &internal_compression)?;
		log::trace!(
			"Root directory bytes uncompressed length: {}",
			root_bytes_uncompressed.len()
		);
		log::trace!("Leaf directories bytes length: {}", leaves_bytes.len());

		// Populate bounds and zoom from the v3 header if the embedded TileJSON omits them.
		if tilejson.bounds.is_none() {
			tilejson.bounds = GeoBBox::new(
				f64::from(header.min_lon_e7) / 1e7,
				f64::from(header.min_lat_e7) / 1e7,
				f64::from(header.max_lon_e7) / 1e7,
				f64::from(header.max_lat_e7) / 1e7,
			)
			.ok();
		}
		if tilejson.zoom_min().is_none() {
			tilejson.set_zoom_min(header.min_zoom);
		}
		if tilejson.zoom_max().is_none() {
			tilejson.set_zoom_max(header.max_zoom);
		}

		let metadata = TileSourceMetadata::new(
			header.tile_type.as_value()?,
			header.tile_compression.as_value()?,
			Traversal {
				order: TraversalOrder::PMTiles,
				size: TraversalSize::new_default(),
			},
			None,
		);
		log::trace!("Reader parameters: {metadata:?}");

		let root_entries = Arc::new(EntriesV3::from_blob(&root_bytes_uncompressed)?);

		Ok(PMTilesReader {
			data_reader: Arc::new(data_reader),
			header,
			internal_compression,
			leaves_bytes: Arc::new(leaves_bytes),
			leaves_cache: Cache::builder()
				.max_capacity(100_000_000)
				.weigher(|_k, v: &Arc<EntriesV3>| {
					// Approximate byte weight. EntriesV3 stores Vec<EntryV3>; the exact
					// struct size isn't directly exposed, so over-estimate via len() * 32B
					// (close to EntryV3's actual size). The weighted cap stays meaningful.
					let bytes = v
						.len()
						.saturating_mul(32)
						.saturating_add(size_of::<ByteRange>())
						.saturating_add(size_of::<Arc<EntriesV3>>());
					u32::try_from(bytes).unwrap_or(u32::MAX)
				})
				.build(),
			tilejson,
			metadata,
			root_bytes_uncompressed,
			root_entries,
			runtime,
		})
	}

	/// Decode and return the root directory entries (`EntriesV3`).
	#[context("reading PMTiles root entries")]
	pub fn tile_entries(&self) -> Result<EntriesV3> {
		EntriesV3::from_blob(&self.root_bytes_uncompressed)
	}

	/// Internal helper to look up a tile by its Hilbert index.
	///
	/// Traverses up to three levels of PMTiles directories to locate the tile data.
	/// Returns `Ok(None)` if the tile does not exist in the directory structure,
	/// and an error if its entry is nested deeper than that.
	#[expect(
		clippy::too_many_arguments,
		reason = "a private helper threading the directory-traversal state; grouping it would only move the list"
	)]
	async fn lookup_tile_by_id(
		tile_id: u64,
		data_reader: &DataReader,
		root_entries: Arc<EntriesV3>,
		leaves_cache: &Cache<ByteRange, Arc<EntriesV3>>,
		leaves_bytes: &Arc<Blob>,
		tile_data_offset: u64,
		tile_compression: &TileCompression,
		tile_format: &TileFormat,
		internal_compression: &TileCompression,
	) -> Result<Option<Tile>> {
		let mut entries = root_entries;

		for _depth in 0..MAX_DIRECTORY_DEPTH {
			let Some(entry) = entries.find_tile(tile_id) else {
				return Ok(None);
			};

			if entry.range.length > 0 {
				if entry.run_length > 0 {
					return Ok(Some(Tile::from_blob(
						data_reader
							.read_range(&entry.range.shifted_forward(tile_data_offset)?)
							.await?,
						*tile_compression,
						*tile_format,
					)));
				}
				entries = Self::load_leaf(entry.range, leaves_cache, leaves_bytes, *internal_compression).await?;
			} else {
				return Ok(None);
			}
		}
		Err(too_deep())
	}

	/// Resolve a tile's byte range by Hilbert index without reading tile data.
	///
	/// Like `lookup_tile_by_id` but returns only the `ByteRange` (shifted to the
	/// tile data section) instead of reading and wrapping the blob. Used by
	/// `get_chunks` to collect ranges before coalescing into bulk reads.
	async fn resolve_tile_range(
		tile_id: u64,
		root_entries: Arc<EntriesV3>,
		leaves_cache: &Cache<ByteRange, Arc<EntriesV3>>,
		leaves_bytes: &Arc<Blob>,
		tile_data_offset: u64,
		internal_compression: TileCompression,
	) -> Result<Option<ByteRange>> {
		let mut entries = root_entries;

		for _depth in 0..MAX_DIRECTORY_DEPTH {
			let Some(entry) = entries.find_tile(tile_id) else {
				return Ok(None);
			};

			if entry.range.length == 0 {
				return Ok(None);
			}

			if entry.run_length > 0 {
				return Ok(Some(entry.range.shifted_forward(tile_data_offset)?));
			}

			entries = Self::load_leaf(entry.range, leaves_cache, leaves_bytes, internal_compression).await?;
		}

		Err(too_deep())
	}

	/// A leaf directory's entries, decompressed once and then taken from the cache.
	async fn load_leaf(
		range: ByteRange,
		leaves_cache: &Cache<ByteRange, Arc<EntriesV3>>,
		leaves_bytes: &Arc<Blob>,
		internal_compression: TileCompression,
	) -> Result<Arc<EntriesV3>> {
		let leaves = Arc::clone(leaves_bytes);
		leaves_cache
			.try_get_with(range, async move {
				let mut blob = leaves.read_range(&range)?;
				blob = decompress(blob, &internal_compression)?;
				anyhow::Ok(Arc::new(EntriesV3::from_blob(&blob)?))
			})
			.await
			.map_err(|e: Arc<anyhow::Error>| anyhow::anyhow!("{e:#}"))
	}

	/// Every tile whose ID lies in `lo..=hi`, with the byte range of its data,
	/// found by walking the directories once rather than looking up each ID.
	///
	/// Entries are sorted by tile ID, so a directory is left at the first entry
	/// past `hi`, and a leaf directory is only opened when the IDs it holds,
	/// up to the next entry's, reach into the range. A run of tiles sharing one
	/// entry is expanded only as far as it overlaps the range.
	async fn tiles_in_id_range(
		lo: u64,
		hi: u64,
		root_entries: Arc<EntriesV3>,
		leaves_cache: &Cache<ByteRange, Arc<EntriesV3>>,
		leaves_bytes: &Arc<Blob>,
		tile_data_offset: u64,
		internal_compression: TileCompression,
	) -> Result<Vec<(u64, ByteRange)>> {
		let mut tiles = Vec::new();
		let mut pending = vec![(root_entries, 0usize)];

		while let Some((entries, depth)) = pending.pop() {
			if depth >= MAX_DIRECTORY_DEPTH {
				return Err(too_deep());
			}
			let entries: Vec<_> = entries.iter().copied().collect();
			for (index, entry) in entries.iter().enumerate() {
				if entry.tile_id > hi {
					break;
				}
				if entry.range.length == 0 {
					continue;
				}
				if entry.run_length > 0 {
					let last = entry
						.tile_id
						.saturating_add(u64::from(entry.run_length.saturating_sub(1)));
					if last < lo {
						continue;
					}
					let range = entry.range.shifted_forward(tile_data_offset)?;
					for id in entry.tile_id.max(lo)..=last.min(hi) {
						tiles.push((id, range));
					}
				} else if entries
					.get(index..)
					.and_then(|rest| rest.get(1))
					.is_none_or(|next| next.tile_id > lo)
				{
					let leaf = Self::load_leaf(entry.range, leaves_cache, leaves_bytes, internal_compression).await?;
					pending.push((leaf, depth.saturating_add(1)));
				}
			}
		}

		Ok(tiles)
	}

	/// The size of each tile in `bbox`, by looking up every coordinate.
	///
	/// Cheapest for a bbox with few coordinates in a file with many tiles.
	async fn sizes_by_lookup(&self, bbox: &TileBBox) -> Result<Vec<(TileCoord, u32)>> {
		let mut sizes = Vec::new();
		let coords: Vec<TileCoord> = bbox.iter_coords().collect();
		for coord in coords {
			let Ok(tile_id) = coord.get_hilbert_index() else {
				continue;
			};
			if let Some(range) = Self::resolve_tile_range(
				tile_id,
				Arc::clone(&self.root_entries),
				&self.leaves_cache,
				&self.leaves_bytes,
				self.header.tile_data.offset,
				self.internal_compression,
			)
			.await? && let Ok(size) = u32::try_from(range.length)
			{
				sizes.push((coord, size));
			}
		}
		Ok(sizes)
	}

	/// The size of each tile in `bbox`, by walking the directory entries whose
	/// IDs could belong to it.
	///
	/// Cheapest for a bbox with many coordinates and few tiles, such as a whole
	/// level of a sparse file, which the lookups would visit coordinate by
	/// coordinate.
	async fn sizes_by_walk(&self, bbox: &TileBBox) -> Result<Vec<(TileCoord, u32)>> {
		let (lo, hi) = hilbert_id_range(bbox)?;
		let tiles = Self::tiles_in_id_range(
			lo,
			hi,
			Arc::clone(&self.root_entries),
			&self.leaves_cache,
			&self.leaves_bytes,
			self.header.tile_data.offset,
			self.internal_compression,
		)
		.await?;

		let mut sizes = Vec::new();
		for (tile_id, range) in tiles {
			let coord = TileCoord::from_hilbert_index(tile_id)?;
			if bbox.includes_coord(&coord)
				&& let Ok(size) = u32::try_from(range.length)
			{
				sizes.push((coord, size));
			}
		}
		Ok(sizes)
	}

	/// Build read chunks by resolving tile byte ranges and coalescing nearby ones.
	async fn get_chunks(&self, bbox: TileBBox) -> Result<Chunks> {
		let mut tile_ranges: Vec<(TileCoord, ByteRange)> = Vec::new();

		// Collect coords first so the non-Send iterator is not held across await.
		let coords: Vec<TileCoord> = bbox.iter_coords().collect();
		for coord in coords {
			let Ok(tile_id) = coord.get_hilbert_index() else {
				continue;
			};
			if let Some(range) = Self::resolve_tile_range(
				tile_id,
				Arc::clone(&self.root_entries),
				&self.leaves_cache,
				&self.leaves_bytes,
				self.header.tile_data.offset,
				self.internal_compression,
			)
			.await?
			{
				tile_ranges.push((coord, range));
			}
		}

		Ok(Chunks::from_tile_ranges(tile_ranges))
	}
}

/// The tile IDs the tiles of `bbox` can have, as one range: those of every
/// descendant of the smallest quadtree tile that contains the bbox.
///
/// The Hilbert curve visits all descendants of a quadtree tile in one unbroken
/// run, ordered as their ancestors are, so at level `z` the descendants of
/// the tile at position `p` of level `k` are the `4^(z-k)` IDs starting at
/// `p · 4^(z-k)` past the first ID of level `z`. The range is exact for a bbox
/// that is such a tile, such as a whole level, and never misses a tile of any
/// other bbox.
fn hilbert_id_range(bbox: &TileBBox) -> Result<(u64, u64)> {
	/// `4^n`: how many tiles a level `n` levels below a tile has under it.
	fn tiles_below(n: u8) -> Result<u64> {
		u32::from(n)
			.checked_mul(2)
			.and_then(|bits| 1u64.checked_shl(bits))
			.context("zoom level too deep for a tile ID")
	}

	/// The first ID of level `z`: the IDs of all levels before it, `(4^z - 1) / 3`.
	fn level_start(z: u8) -> Result<u64> {
		tiles_below(z)?
			.checked_sub(1)
			.and_then(|before| before.checked_div(3))
			.context("zoom level too deep for a tile ID")
	}

	let level = bbox.level();
	let (x_min, y_min, x_max, y_max) = (bbox.x_min()?, bbox.y_min()?, bbox.x_max()?, bbox.y_max()?);
	let mut shift = 0u8;
	while shift < level && ((x_min >> shift) != (x_max >> shift) || (y_min >> shift) != (y_max >> shift)) {
		// At most `level`, which is below 32.
		shift = shift.saturating_add(1);
	}

	let ancestor_level = level.checked_sub(shift).context("the ancestor lies above level 0")?;
	let ancestor = TileCoord::new(ancestor_level, x_min >> shift, y_min >> shift)?;
	let block = tiles_below(shift)?;
	let lo = ancestor
		.get_hilbert_index()?
		.checked_sub(level_start(ancestor_level)?)
		.and_then(|position| position.checked_mul(block))
		.zip(Some(level_start(level)?))
		.and_then(|(offset, start)| start.checked_add(offset))
		.context("tile ID out of range")?;
	let hi = block
		.checked_sub(1)
		.and_then(|last| lo.checked_add(last))
		.context("tile ID out of range")?;
	Ok((lo, hi))
}

/// Build the per‑zoom tile pyramid by traversing `PMTiles` directory entries.
///
/// Walks the root and leaf directory blobs, following entry ranges. For `run_length`
/// entries, expands the run into individual tiles via Hilbert indices; for directory
/// entries, decompresses and recurses. Returns the accumulated [`TilePyramid`].
///
/// ### Parameters
/// - `root_bytes_uncompressed`: uncompressed root directory bytes.
/// - `leaves_bytes`: concatenated (compressed) leaf directory bytes as a single blob.
/// - `compression`: compression algorithm used for directory blobs.
/// - `addressed_tiles_count`: the header's own count of addressed tiles, used as
///   the ceiling on how many the walk may expand. `0` means the writer left it
///   unset, in which case [`MAX_UNDECLARED_ADDRESSED_TILES`] applies instead.
/// - `progress`: advanced to the number of tiles walked after each directory.
///
/// ### Errors
/// Returns an error when directory blobs cannot be parsed or decompressed, or
/// when the directories address more tiles than the header declares.
#[context("building tile pyramid from PMTiles directories")]
fn calc_tile_pyramid(
	root_bytes_uncompressed: &Blob,
	leaves_bytes: &Blob,
	compression: TileCompression,
	addressed_tiles_count: u64,
	progress: Option<&ProgressHandle>,
) -> Result<TilePyramid> {
	calc_tile_pyramid_capped(
		root_bytes_uncompressed,
		leaves_bytes,
		compression,
		addressed_tiles_count,
		MAX_UNDECLARED_ADDRESSED_TILES,
		progress,
	)
}

/// [`calc_tile_pyramid`] with the cap as a parameter, so a test can reach it
/// without expanding four billion tiles.
fn calc_tile_pyramid_capped(
	root_bytes_uncompressed: &Blob,
	leaves_bytes: &Blob,
	compression: TileCompression,
	addressed_tiles_count: u64,
	cap: u64,
	progress: Option<&ProgressHandle>,
) -> Result<TilePyramid> {
	// The header's count is itself untrusted, so it bounds the walk rather than
	// being believed: a file may declare fewer tiles than it addresses and be
	// rejected, but it cannot declare more than the cap below and use that as a
	// licence to spin.
	//
	// The error names which of the two bounds was hit. It used to blame the
	// header even when the header declared no count, or more than the cap.
	let (limit, reason) = if addressed_tiles_count == 0 {
		(
			cap,
			format!("the header declares no count, and this reader expands at most {cap} without one"),
		)
	} else if addressed_tiles_count > cap {
		(
			cap,
			format!("the header declares {addressed_tiles_count}, more than the {cap} this reader expands"),
		)
	} else {
		(
			addressed_tiles_count,
			String::from("which the header does not account for"),
		)
	};

	let mut walk = DirectoryWalk {
		pyramid: TilePyramid::new_empty(),
		total_entries: 0,
		limit,
		leaves_bytes,
		compression,
		progress,
	};
	walk.parse(root_bytes_uncompressed, 0).map_err(|e| {
		if e.is::<TooManyTiles>() {
			anyhow::anyhow!("PMTiles directories address more than {limit} tiles: {reason}")
		} else {
			e
		}
	})?;

	Ok(walk.pyramid)
}

/// The state of [`calc_tile_pyramid_capped`]'s walk, which stays the same
/// through every level of the recursion.
struct DirectoryWalk<'a> {
	pyramid: TilePyramid,
	total_entries: u64,
	limit: u64,
	leaves_bytes: &'a Blob,
	compression: TileCompression,
	progress: Option<&'a ProgressHandle>,
}

impl DirectoryWalk<'_> {
	fn parse(&mut self, dir: &Blob, depth: usize) -> Result<()> {
		log::trace!("parse_directories");

		if depth >= MAX_DIRECTORY_DEPTH {
			return Err(too_deep());
		}

		let entries = EntriesV3::from_blob(dir)?;

		for entry in entries.iter() {
			if entry.range.length > 0 {
				if entry.run_length > 0 {
					// `run_length` is a `u32` costing one varint byte in the file,
					// so a handful of bytes can ask for billions of tiles. Folding
					// each coordinate straight into the pyramid keeps that
					// bounded in memory; the running total keeps it bounded in
					// time.
					self.total_entries = self.total_entries.saturating_add(u64::from(entry.run_length));
					if self.total_entries > self.limit {
						return Err(TooManyTiles.into());
					}

					for i in 0..u64::from(entry.run_length) {
						let tile_id = i
							.checked_add(entry.tile_id)
							.context("tile ids in the PMTiles directory overflow u64")?;
						self.pyramid.insert_coord(&TileCoord::from_hilbert_index(tile_id)?);
					}
				} else {
					let range = entry.range;
					let mut blob = self.leaves_bytes.read_range(&range)?;
					blob = decompress(blob, &self.compression)?;
					#[expect(
						clippy::arithmetic_side_effects,
						reason = "`depth` is below MAX_DIRECTORY_DEPTH, checked above"
					)]
					self.parse(&blob, depth + 1)?;
				}
			}
		}

		// Once per directory rather than per tile: every update takes the
		// progress handle's lock, and a directory holds thousands of entries.
		if let Some(progress) = self.progress {
			progress.set_position(self.total_entries);
		}

		Ok(())
	}
}

#[async_trait]
impl TilesReader for PMTilesReader {
	async fn open_reader(reader: DataReader, runtime: TilesRuntime) -> Result<SharedTileSource> {
		Ok(Self::open_data(reader, runtime).await?.into_shared())
	}
}

#[async_trait]
impl TileSource for PMTilesReader {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("pmtiles", self.data_reader.name())
	}

	/// Returns the current reader parameters (tile format, compression, tile pyramid).
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	/// Returns the parsed and merged `TileJSON` metadata.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	/// Returns the coverage pyramid, computing it on first access.
	///
	/// The first call walks every directory entry and every tile it addresses,
	/// which for a planet-sized file takes a while, and reports how far it has
	/// got through a progress handle counting up to the header's tile count
	/// (#305).
	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self.metadata.get_or_compute_tile_pyramid(|| {
			let progress = self
				.runtime
				.create_progress("computing the tile pyramid", self.header.addressed_tiles_count);
			let pyramid = calc_tile_pyramid(
				&self.root_bytes_uncompressed,
				&self.leaves_bytes,
				self.internal_compression,
				self.header.addressed_tiles_count,
				Some(&progress),
			);
			progress.finish();
			pyramid
		})
	}

	/// Fetch a tile by XYZ coordinate.
	///
	/// Converts the coordinate to a **Hilbert tile ID**, then traverses up to three levels
	/// of `PMTiles` directories to locate the tile. Leaf directories are cached to avoid
	/// repeated decompression. Returns `Ok(None)` if the tile does not exist.
	#[context("fetching tile {:?} from PMTiles", coord)]
	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		log::trace!("tile {coord:?}");
		let tile_id = coord.get_hilbert_index()?;
		Self::lookup_tile_by_id(
			tile_id,
			&self.data_reader,
			Arc::clone(&self.root_entries),
			&self.leaves_cache,
			&self.leaves_bytes,
			self.header.tile_data.offset,
			self.metadata.tile_compression(),
			self.metadata.tile_format(),
			&self.internal_compression,
		)
		.await
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("pmtiles::tile_stream {bbox:?}");
		let bbox = self.metadata.intersection_bbox(&bbox);
		let chunks = self.get_chunks(bbox).await?;
		Ok(chunks.stream(
			Arc::clone(&self.data_reader),
			*self.metadata.tile_compression(),
			*self.metadata.tile_format(),
			&self.runtime,
		))
	}

	#[context("streaming tile sizes for bbox {:?}", bbox)]
	/// The sizes from the directories, without reading a tile.
	///
	/// Two ways to get them, and this picks the cheaper. Looking up each
	/// coordinate costs one lookup per coordinate of the bbox, whether a tile is
	/// there or not: a whole level at z14 is 268 million lookups. Walking the
	/// directory entries costs at most one step per tile whose ID could belong
	/// to the bbox, which is never more than the tiles in the file.
	async fn tile_size_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, u32>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		if bbox.is_empty() {
			return Ok(TileStream::from_vec(Vec::new()));
		}

		let (lo, hi) = hilbert_id_range(&bbox)?;
		let tiles_in_file = match self.header.addressed_tiles_count {
			0 => u64::MAX,
			count => count,
		};
		let walk_steps = hi.saturating_sub(lo).saturating_add(1).min(tiles_in_file);

		let tile_sizes = if bbox.count_tiles() <= walk_steps {
			self.sizes_by_lookup(&bbox).await?
		} else {
			self.sizes_by_walk(&bbox).await?
		};
		Ok(TileStream::from_vec(tile_sizes))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		Ok(self.tile_size_stream(bbox).await?.filter_map(|_, _| Some(())))
	}

	// deep probe of container meta
	#[cfg(feature = "cli")]
	/// Adds PMTiles‑specific container metadata (the v3 header) to the CLI probe output.
	///
	/// Printed under the `"header"` key for human‑readable inspection.
	#[context("probing PMTiles container metadata")]
	async fn probe_container(&self, print: &mut PrettyPrint, _runtime: &TilesRuntime) -> Result<()> {
		let h = &self.header;
		print
			.add_key_value("addressed tiles count", &h.addressed_tiles_count)
			.await;
		print.add_key_value("tile entries count", &h.tile_entries_count).await;
		print.add_key_value("tile contents count", &h.tile_contents_count).await;
		print.add_key_value("clustered", &h.clustered).await;
		print
			.add_key_value("internal compression", &h.internal_compression)
			.await;
		print.add_key_value("tile type", &h.tile_type).await;
		print
			.add_key_value("zoom range", &format!("{}..{}", h.min_zoom, h.max_zoom))
			.await;
		print.add_key_value("root dir size", &h.root_dir.length).await;
		print.add_key_value("metadata size", &h.metadata.length).await;
		print.add_key_value("leaf dirs size", &h.leaf_dirs.length).await;
		print.add_key_value("tile data size", &h.tile_data.length).await;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use std::{env::current_dir, path::PathBuf, sync::LazyLock};

	use versatiles_core::assert_wildcard;

	use super::*;

	static PATH: LazyLock<PathBuf> = LazyLock::new(|| current_dir().unwrap().join("../testdata/berlin.pmtiles"));

	/// Serializes one leaf-pointer entry (`run_length == 0`) covering `length`
	/// bytes from the start of the leaf blob.
	fn leaf_pointer_dir(length: u64) -> Result<Blob> {
		let mut entries = super::super::types::EntriesV3::new();
		entries.push(super::super::types::EntryV3::new(0, ByteRange::new(0, length), 0));
		entries.as_slice().serialize_entries()
	}

	/// Computing the pyramid walks every tile and used to say nothing while it
	/// did. It now reports progress up to the header's tile count (#305).
	#[tokio::test]
	async fn computing_the_pyramid_reports_progress() -> Result<()> {
		let runtime = TilesRuntime::new_silent();
		let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
		let sink = Arc::clone(&seen);
		runtime.events().subscribe(move |event| {
			if let crate::Event::Progress { data } = event
				&& data.message == "computing the tile pyramid"
			{
				sink.lock().unwrap().push((data.position, data.total, data.finished));
			}
		});

		let reader = PMTilesReader::open(&PATH, runtime).await?;
		let total = reader.header.addressed_tiles_count;
		assert!(total > 0);
		reader.tile_pyramid().await?;
		assert_eq!(seen.lock().unwrap().last(), Some(&(total, total, true)));

		// Only on the first call: the second returns the stored pyramid.
		let events = seen.lock().unwrap().len();
		reader.tile_pyramid().await?;
		assert_eq!(seen.lock().unwrap().len(), events);
		Ok(())
	}

	/// A leaf directory entry may name the very bytes it was parsed from. The
	/// walk is recursive, so that is unbounded recursion: the stack goes, and a
	/// stack overflow aborts the process rather than unwinding, so no caller can
	/// catch it. It must be a plain error instead.
	#[test]
	fn a_self_referential_leaf_directory_is_refused() -> Result<()> {
		// The entry has to describe the blob that contains it, so serialize once
		// to learn the length, then again with that length filled in.
		let mut blob = leaf_pointer_dir(5)?;
		for _ in 0..4 {
			let next = leaf_pointer_dir(blob.len())?;
			if next.len() == blob.len() {
				blob = next;
				break;
			}
			blob = next;
		}

		// Root and leaves are the same bytes: following the entry re-reads the
		// directory currently being parsed.
		let error = calc_tile_pyramid(&blob, &blob, TileCompression::Uncompressed, 0, None)
			.expect_err("a self-referential leaf directory must be an error, not an abort");

		let message = format!("{error:#}");
		assert!(
			message.contains("nest more than 3 levels deep"),
			"unhelpful message: {message}"
		);

		Ok(())
	}

	/// The ID range of a bbox, checked against every tile in it. An aligned
	/// quadtree tile's descendants must be exactly its range; any other bbox
	/// must not have a tile outside it.
	#[test]
	fn the_id_range_of_a_bbox_holds_every_tile_of_it() -> Result<()> {
		for level in 0..=6u8 {
			let size = 1u32 << level;
			let mut bboxes = vec![TileBBox::new_full(level)?];
			for (x0, y0, x1, y1) in [(0, 0, 0, 0), (1, 0, 2, 1), (0, 1, 3, 3), (2, 2, 5, 6), (3, 0, 3, 7)] {
				if x1 < size && y1 < size {
					bboxes.push(TileBBox::from_min_and_max(level, x0, y0, x1, y1)?);
				}
			}
			for quad_level in 0..=level {
				let side = 1u32 << (level - quad_level);
				for qx in 0..(1u32 << quad_level).min(4) {
					let (x0, y0) = (qx * side, (qx % 2) * side);
					bboxes.push(TileBBox::from_min_and_max(level, x0, y0, x0 + side - 1, y0 + side - 1)?);
				}
			}

			for bbox in bboxes {
				let (lo, hi) = hilbert_id_range(&bbox)?;
				let ids: Vec<u64> = bbox.iter_coords().map(|c| c.get_hilbert_index().unwrap()).collect();
				assert!(
					ids.iter().all(|id| (lo..=hi).contains(id)),
					"{bbox:?} has a tile outside {lo}..={hi}"
				);

				let aligned = bbox.count_tiles().is_power_of_two()
					&& bbox.width() == bbox.height()
					&& bbox.x_min()? % bbox.width() == 0
					&& bbox.y_min()? % bbox.height() == 0;
				if aligned {
					assert_eq!(hi - lo + 1, bbox.count_tiles(), "{bbox:?} is a quadtree tile");
				}
			}
		}
		Ok(())
	}

	/// Walking the directory entries and looking up each coordinate are two ways to
	/// the same sizes, for whole levels and for parts of them.
	#[tokio::test]
	async fn walking_the_directories_gives_the_sizes_the_lookups_give() -> Result<()> {
		let reader = PMTilesReader::open(&PATH, TilesRuntime::default()).await?;
		let pyramid = reader.tile_pyramid().await?;

		let mut bboxes = Vec::new();
		for level in pyramid.level_min().unwrap_or(0)..=pyramid.level_max().unwrap_or(0) {
			bboxes.push(TileBBox::new_full(level)?);
			let tiles = pyramid.level_bbox(level);
			if !tiles.is_empty() {
				bboxes.push(tiles);
				let (x, y) = (tiles.x_min()?, tiles.y_min()?);
				let max = (1u32 << level) - 1;
				bboxes.push(TileBBox::from_min_and_max(
					level,
					x,
					y,
					(x + 1).min(max),
					(y + 2).min(max),
				)?);
			}
		}

		let mut compared = 0;
		for bbox in bboxes {
			let bbox = reader.metadata().intersection_bbox(&bbox);
			if bbox.is_empty() {
				continue;
			}
			let mut by_walk = reader.sizes_by_walk(&bbox).await?;
			let mut by_lookup = reader.sizes_by_lookup(&bbox).await?;
			by_walk.sort_by_key(|(c, _)| (c.y, c.x));
			by_lookup.sort_by_key(|(c, _)| (c.y, c.x));
			assert_eq!(by_walk, by_lookup, "{bbox:?}");
			compared += by_walk.len();
		}
		assert!(compared > 0, "the comparison saw no tiles");
		Ok(())
	}

	/// The same self-referential directory, met by the tile lookups and the walk.
	/// One lookup used to say "not found" without context, the other to drop the
	/// tile as a gap; all three now say what the pyramid walk says.
	#[tokio::test]
	async fn the_tile_lookups_refuse_a_self_referential_leaf_directory() -> Result<()> {
		let mut blob = leaf_pointer_dir(5)?;
		for _ in 0..4 {
			let next = leaf_pointer_dir(blob.len())?;
			let done = next.len() == blob.len();
			blob = next;
			if done {
				break;
			}
		}
		let root = Arc::new(super::super::types::EntriesV3::from_blob(&blob)?);
		let leaves = Arc::new(blob.clone());
		let cache = Cache::builder().build();
		let data_reader: DataReader = Box::new(versatiles_core::io::DataReaderBlob::from(blob));

		let range =
			PMTilesReader::resolve_tile_range(0, Arc::clone(&root), &cache, &leaves, 0, TileCompression::Uncompressed)
				.await
				.expect_err("the stream path must not drop the tile as a gap");
		assert!(
			format!("{range:#}").contains("nest more than 3 levels deep"),
			"{range:#}"
		);

		let tile = PMTilesReader::lookup_tile_by_id(
			0,
			&data_reader,
			Arc::clone(&root),
			&cache,
			&leaves,
			0,
			&TileCompression::Uncompressed,
			&TileFormat::MVT,
			&TileCompression::Uncompressed,
		)
		.await
		.expect_err("the tile path must say why");
		assert!(format!("{tile:#}").contains("nest more than 3 levels deep"), "{tile:#}");

		let walk = PMTilesReader::tiles_in_id_range(0, 100, root, &cache, &leaves, 0, TileCompression::Uncompressed)
			.await
			.expect_err("the walk must neither loop nor drop the tiles");
		assert!(format!("{walk:#}").contains("nest more than 3 levels deep"), "{walk:#}");
		Ok(())
	}

	/// `run_length` is a `u32` costing one varint byte, so a few bytes can ask the
	/// walk to expand billions of tiles. It is bounded by what the header says the
	/// archive addresses.
	#[test]
	fn a_run_length_beyond_the_declared_tile_count_is_refused() -> Result<()> {
		let mut entries = super::super::types::EntriesV3::new();
		entries.push(super::super::types::EntryV3::new(0, ByteRange::new(0, 10), 50_000_000));
		let blob = entries.as_slice().serialize_entries()?;

		// The header declares eight addressed tiles; the directory asks for 50M.
		let error = calc_tile_pyramid(&blob, &Blob::default(), TileCompression::Uncompressed, 8, None)
			.expect_err("a run length beyond the declared count must be an error");

		let message = format!("{error:#}");
		assert!(message.contains("address more than 8 tiles"), "unhelpful: {message}");

		Ok(())
	}

	/// Without a declared count the cap applies, and the error says so rather
	/// than blaming the header.
	#[test]
	fn the_cap_without_a_declared_count_is_named() -> Result<()> {
		let mut entries = super::super::types::EntriesV3::new();
		entries.push(super::super::types::EntryV3::new(0, ByteRange::new(0, 10), 50));
		let blob = entries.as_slice().serialize_entries()?;

		let error = calc_tile_pyramid_capped(&blob, &Blob::default(), TileCompression::Uncompressed, 0, 8, None)
			.expect_err("more than the cap must be an error");
		assert!(
			format!("{error:#}").contains("declares no count"),
			"unhelpful: {error:#}"
		);

		let error = calc_tile_pyramid_capped(&blob, &Blob::default(), TileCompression::Uncompressed, 100, 8, None)
			.expect_err("more than the cap must be an error");
		assert!(
			format!("{error:#}").contains("declares 100, more than the 8"),
			"unhelpful: {error:#}"
		);
		Ok(())
	}

	/// A run that fits the declared count is expanded as before.
	#[test]
	fn a_run_length_within_the_declared_tile_count_is_expanded() -> Result<()> {
		let mut entries = super::super::types::EntriesV3::new();
		entries.push(super::super::types::EntryV3::new(0, ByteRange::new(0, 10), 4));
		let blob = entries.as_slice().serialize_entries()?;

		let pyramid = calc_tile_pyramid(&blob, &Blob::default(), TileCompression::Uncompressed, 4, None)?;
		assert!(!pyramid.is_empty());

		Ok(())
	}

	/// The bound must not refuse the nesting the format actually produces.
	#[test]
	fn a_root_directory_without_leaves_still_walks() -> Result<()> {
		let mut entries = super::super::types::EntriesV3::new();
		entries.push(super::super::types::EntryV3::new(0, ByteRange::new(0, 10), 1));
		let blob = entries.as_slice().serialize_entries()?;

		let pyramid = calc_tile_pyramid(&blob, &Blob::default(), TileCompression::Uncompressed, 0, None)?;
		assert!(!pyramid.is_empty());

		Ok(())
	}

	#[tokio::test]
	async fn reader() -> Result<()> {
		let reader = PMTilesReader::open(&PATH, TilesRuntime::default()).await?;

		assert_wildcard!(
			reader.source_type().to_string(),
			"container 'pmtiles' ('*testdata?berlin.pmtiles')"
		);

		assert_eq!(
			format!("{:?}", reader.header),
			"HeaderV3 { root_dir: ByteRange[127,504], metadata: ByteRange[631,888], leaf_dirs: ByteRange[1519,0], tile_data: ByteRange[1519,11339914], addressed_tiles_count: 130, tile_entries_count: 130, tile_contents_count: 130, clustered: true, internal_compression: Gzip, tile_compression: Gzip, tile_type: MVT, min_zoom: 0, max_zoom: 14, min_lon_e7: 133000000, min_lat_e7: 524500000, max_lon_e7: 134600000, max_lat_e7: 525500000, center_zoom: 2, center_lon_e7: 133813476, center_lat_e7: 525028064 }"
		);

		assert_wildcard!(
			reader.tilejson().stringify(),
			"{\"author\":\"OpenStreetMap contributors\",*,\"version\":\"2.0\"}"
		);

		assert_eq!(
			format!("{:?}", reader.metadata()),
			"TileSourceMetadata { tile_compression: Gzip, tile_format: MVT, traversal: Traversal(PMTiles,full), preferred_traversal: None, tile_pyramid: RwLock { data: None, poisoned: false, .. } }"
		);

		assert_eq!(
			reader
				.tile(&TileCoord::new(0, 0, 0)?)
				.await?
				.unwrap()
				.as_blob(reader.metadata.tile_compression())?
				.len(),
			90891
		);

		assert_eq!(
			reader
				.tile(&TileCoord::new(14, 8800, 5370)?)
				.await?
				.unwrap()
				.as_blob(reader.metadata.tile_compression())?
				.len(),
			130653
		);

		assert!(reader.tile(&TileCoord::new(16, 0, 0)?).await?.is_none());

		Ok(())
	}

	#[tokio::test]
	async fn satisfies_stream_count_invariant() -> Result<()> {
		let reader = PMTilesReader::open(&PATH, TilesRuntime::default()).await?;
		let pyr = reader.tile_pyramid().await?;
		for z in 0..=pyr.level_max().unwrap_or(0) {
			let bbox = pyr.level_ref(z).to_bbox();
			if bbox.is_empty() {
				continue;
			}
			crate::testing::assert_stream_counts_agree(&reader, bbox).await?;
		}
		Ok(())
	}

	#[tokio::test]
	async fn tile_size_stream_matches_tile_reads() -> Result<()> {
		let reader = PMTilesReader::open(&PATH, TilesRuntime::default()).await?;

		let bbox = TileBBox::from_min_and_max(9, 274, 167, 275, 168)?;
		let compression = reader.metadata().tile_compression();

		let mut sizes: Vec<(TileCoord, u32)> = reader.tile_size_stream(bbox).await?.to_vec().await;
		sizes.sort_by_key(|(c, _)| (c.y, c.x));

		assert_eq!(sizes.len(), 4);

		for (coord, size) in &sizes {
			let blob = reader
				.tile(coord)
				.await?
				.expect("tile should exist")
				.into_blob(compression)?;
			assert_eq!(u64::from(*size), blob.len(), "size mismatch at {coord:?}");
		}

		Ok(())
	}

	#[tokio::test]
	async fn tile_size_stream_single_tile() -> Result<()> {
		let reader = PMTilesReader::open(&PATH, TilesRuntime::default()).await?;

		let bbox = TileBBox::from_min_and_max(0, 0, 0, 0, 0)?;
		let sizes: Vec<(TileCoord, u32)> = reader.tile_size_stream(bbox).await?.to_vec().await;

		assert_eq!(sizes.len(), 1);
		assert_eq!(sizes[0].0, TileCoord::new(0, 0, 0)?);
		assert_eq!(sizes[0].1, 90891);

		Ok(())
	}

	#[tokio::test]
	async fn tile_size_stream_empty_for_missing_zoom() -> Result<()> {
		let reader = PMTilesReader::open(&PATH, TilesRuntime::default()).await?;

		let bbox = TileBBox::from_min_and_max(20, 0, 0, 3, 3)?;
		let sizes: Vec<(TileCoord, u32)> = reader.tile_size_stream(bbox).await?.to_vec().await;

		assert_eq!(sizes, []);

		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn probe() -> Result<()> {
		use versatiles_core::utils::PrettyPrint;

		let reader = PMTilesReader::open(&PATH, TilesRuntime::default()).await?;
		let runtime = TilesRuntime::default();

		let mut printer = PrettyPrint::new();
		reader
			.probe_container(&mut printer.category("container").await, &runtime)
			.await?;
		let output = printer.stringify().await;
		assert!(
			output.contains("addressed tiles count: 130"),
			"unexpected output: {output}"
		);
		assert!(output.contains("clustered: true"), "unexpected output: {output}");
		assert!(output.contains("tile data size:"), "unexpected output: {output}");

		Ok(())
	}

	#[tokio::test]
	async fn concurrent_tile_lookups_share_leaves_cache() -> Result<()> {
		// Verifies the moka cache lets concurrent tile lookups dedup leaf-directory
		// decompression. The test PMTiles file in this fixture has all entries in
		// the root dir (leaf_dirs.length == 0), so the cache is never populated here —
		// but the API path must still work without holding a mutex across the await.
		let reader = Arc::new(PMTilesReader::open(&PATH, TilesRuntime::default()).await?);
		let coord = TileCoord::new(14, 8800, 5370)?;
		let mut handles = Vec::new();
		for _ in 0..16 {
			let r = Arc::clone(&reader);
			handles.push(tokio::spawn(async move { r.tile(&coord).await.unwrap() }));
		}
		for h in handles {
			let tile = h.await.unwrap().expect("tile should exist");
			assert!(!tile.into_blob(&TileCompression::Gzip)?.is_empty());
		}
		Ok(())
	}

	#[tokio::test]
	async fn tile_stream_matches_individual_reads() -> Result<()> {
		let reader = PMTilesReader::open(&PATH, TilesRuntime::default()).await?;

		// Use level 9 bbox which has 2x2 = 4 tiles according to the metadata
		let bbox = TileBBox::from_min_and_max(9, 274, 167, 275, 168)?;

		// Get all tiles via stream
		let stream = reader.tile_stream(bbox).await?;
		let stream_tiles: Vec<_> = stream.to_vec().await;
		assert_eq!(stream_tiles.len(), 4);

		// Verify each streamed tile matches individual read
		for (coord, mut tile) in stream_tiles {
			let stream_blob = tile.as_blob(reader.metadata().tile_compression())?;
			let single_blob = reader
				.tile(&coord)
				.await?
				.expect("tile should exist")
				.into_blob(reader.metadata().tile_compression())?;
			assert_eq!(
				stream_blob.as_slice(),
				single_blob.as_slice(),
				"blob mismatch at {coord:?}"
			);
		}

		Ok(())
	}
}
