//! Read tiles and metadata from a `.versatiles` container.
//!
//! The `VersaTilesReader` parses the container header, decompresses the **block index**,
//! reads embedded `TileJSON` metadata, and exposes tiles via [`TileSource`]. The
//! file format organizes data into fixed **256×256 tile blocks**; each block stores
//! a Brotli-compressed tile index (byte ranges), followed by a contiguous region of
//! tile blobs. This reader lazily caches decoded tile indices for fast random access.
//!
//! ## Extracted artifacts
//! - `tilejson`: parsed `TileJSON` from the `meta_range` (if present)
//! - `parameters`: [`TileSourceMetadata`] with `tile_format`, `tile_compression`, and a
//!   **bbox pyramid** computed from the block index
//! - `block_index`: lightweight structure describing all block ranges
//!
//! ## Usage
//! ```rust,no_run
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use anyhow::Result;
//! use futures::StreamExt;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Open a .versatiles container (relative or absolute path)
//!     let runtime = TilesRuntime::default();
//!     let path = Path::new("./data/world.versatiles");
//!     let mut reader = VersaTilesReader::open_path(&path, runtime).await?;
//!
//!     // Inspect parameters & TileJSON
//!     let metadata = reader.metadata();
//!     let tj = reader.tilejson();
//!     println!("format={:?} compression={:?}", metadata.tile_format, metadata.tile_compression);
//!
//!     // Fetch one tile
//!     if let Some(mut tile) = reader.get_tile(&TileCoord::new(15, 1, 4)?).await? {
//!         let _blob = tile.as_blob(metadata.tile_compression)?;
//!     }
//!
//!     // Stream a bbox (coalesces reads per block for fewer I/O calls)
//!     let bbox = metadata.bbox_pyramid.get_level_bbox(4).clone();
//!     let mut stream = reader.get_tile_stream(bbox).await?;
//!     while let Some((coord, mut tile)) = stream.next().await {
//!         let _size = tile.as_blob(metadata.tile_compression)?.len();
//!         // use (coord, _size)
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Errors
//! Returns errors when the file cannot be read or decompressed, when metadata/index parsing fails,
//! or when a requested tile is missing.

use super::types::{BlockDefinition, BlockIndex, FileHeader, TileIndex};
use crate::{
	SourceType, Tile, TileSource, TileSourceMetadata, TilesRuntime, Traversal, TraversalOrder, TraversalSize,
	container::tile_chunking::{Chunk, coalesce_into_chunks, stream_from_chunks},
};
use anyhow::Result;
use async_trait::async_trait;
use futures::{lock::Mutex, stream::StreamExt};
use std::{fmt::Debug, ops::Shr, path::Path, sync::Arc};
#[cfg(feature = "cli")]
use versatiles_core::utils::PrettyPrint;
use versatiles_core::{
	ByteRange, LimitedCache, TileBBox, TileCoord, TileJSON, TileStream,
	compression::decompress,
	io::{DataReader, DataReaderFile},
};
use versatiles_derive::context;

/// Reader for `.versatiles` containers.
///
/// Decompresses and parses the block index, merges embedded `TileJSON`, computes a
/// per-zoom bounding-box pyramid, and serves tiles via lazy index lookups. Tile
/// indices are cached (least-recently-used) to accelerate repeated random access.
pub struct VersaTilesReader {
	block_index: BlockIndex,
	#[allow(dead_code)] // used by probe_container under #[cfg(feature = "cli")]
	header: FileHeader,
	metadata: TileSourceMetadata,
	reader: Arc<DataReader>,
	tile_index_cache: Mutex<LimitedCache<TileCoord, Arc<TileIndex>>>,
	tilejson: TileJSON,
	#[allow(dead_code)] // used by probe_tiles under #[cfg(feature = "cli")]
	runtime: TilesRuntime,
}

impl VersaTilesReader {
	/// Open a `.versatiles` container from a filesystem path.
	///
	/// Creates a `DataReaderFile` and delegates to [`VersaTilesReader::open_reader`]. The path may be
	/// relative or absolute.
	///
	/// # Errors
	/// Returns an error if the file cannot be opened.
	#[context("Failed to open versatiles file at '{path:?}'")]
	pub async fn open_path(path: &Path, runtime: TilesRuntime) -> Result<VersaTilesReader> {
		VersaTilesReader::open_reader(DataReaderFile::open(path)?, runtime).await
	}

	/// Open a `.versatiles` container from an existing [`DataReader`].
	///
	/// Reads the header, loads and (if present) decompresses the `TileJSON` metadata, then
	/// reads and decompresses the **block index** (Brotli). Finally, computes the bbox pyramid
	/// from the block index and initializes the tile-index cache.
	///
	/// # Errors
	/// Returns an error if header/metadata/index reads or decompressions fail.
	#[context("Failed to open versatiles reader")]
	pub async fn open_reader(mut reader: DataReader, runtime: TilesRuntime) -> Result<VersaTilesReader> {
		let header = FileHeader::from_reader(&mut reader)
			.await
			.context("Failed reading the header")?;

		let tilejson = if header.meta_range.length > 0 {
			let blob = reader
				.read_range(&header.meta_range)
				.await
				.context("Failed reading the meta data")?;
			let blob = decompress(blob, header.compression).context("Failed decompressing the meta data")?;
			TileJSON::try_from_blob_or_default(&blob)
		} else {
			TileJSON::default()
		};

		let block_index_blob = reader
			.read_range(&header.blocks_range)
			.await
			.context("Failed reading the block index")?;
		let block_index =
			BlockIndex::from_brotli_blob(&block_index_blob).context("Failed decompressing the block index")?;

		let bbox_pyramid = block_index.get_bbox_pyramid();
		let metadata = TileSourceMetadata::new(
			header.tile_format,
			header.compression,
			bbox_pyramid,
			Traversal {
				order: TraversalOrder::AnyOrder,
				size: TraversalSize::new_max(256)?,
			},
		);

		Ok(VersaTilesReader {
			block_index,
			header,
			metadata,
			reader: Arc::new(reader),
			tile_index_cache: Mutex::new(LimitedCache::with_maximum_size(100_000_000)),
			tilejson,
			runtime,
		})
	}

	/// Load (and cache) the tile index for a block.
	///
	/// Reads the block's index blob, decompresses it, adjusts offsets to the tiles segment,
	/// and inserts the result into an in-memory LRU-like cache. Subsequent calls reuse the cache.
	///
	/// # Errors
	/// Returns an error if reading or decompression fails.
	#[context("Failed to get tile index for block {block:?}")]
	async fn get_block_tile_index(&self, block: &BlockDefinition) -> Result<Arc<TileIndex>> {
		let block_coord = block.get_coord();

		let mut cache = self.tile_index_cache.lock().await;

		Ok(if let Some(value) = cache.get(block_coord) {
			value
		} else {
			let blob = self.reader.read_range(block.get_index_range()).await?;
			let mut tile_index = TileIndex::from_brotli_blob(&blob)?;
			tile_index.add_offset(block.get_tiles_range().offset);

			debug_assert_eq!(tile_index.len(), usize::try_from(block.count_tiles())?);

			cache.add(*block_coord, Arc::new(tile_index))
		})
	}

	/// Sum of all block index byte lengths.
	#[cfg(feature = "cli")]
	fn get_index_size(&self) -> u64 {
		self.block_index.iter().map(|b| b.get_index_range().length).sum()
	}

	/// Sum of all block tiles byte lengths.
	fn get_tiles_size(&self) -> u64 {
		self.block_index.iter().map(|b| b.get_tiles_range().length).sum()
	}

	/// Build read **chunks** by grouping tile ranges within the same block.
	///
	/// Coalesces nearby ranges into at most ~64 MiB chunks (with a small gap tolerance)
	/// to minimize I/O calls during streaming.
	async fn get_chunks(&self, bbox: TileBBox) -> Result<Vec<Chunk>> {
		let block_coords: Vec<TileCoord> = bbox.scaled_down(256).iter_coords().collect();

		let stream = futures::stream::iter(block_coords).then(|block_coord: TileCoord| {
			async move {
				// Get the block using the block coordinate
				let Some(block) = self.block_index.get_block(&block_coord) else {
					return Ok(Vec::new());
				};
				let block = block.clone();
				log::trace!("block {block:?}");

				// Get the bounding box of all tiles defined in this block
				let tiles_bbox_block = block.get_global_bbox();
				log::trace!("tiles_bbox_block {tiles_bbox_block:?}");

				// Get the bounding box of all tiles defined in this block
				let mut tiles_bbox_used: TileBBox = bbox;
				tiles_bbox_used.intersect_with(tiles_bbox_block)?;
				log::trace!("tiles_bbox_used {tiles_bbox_used:?}");

				debug_assert_eq!(bbox.level, tiles_bbox_block.level);
				debug_assert_eq!(bbox.level, tiles_bbox_used.level);

				// Get the tile index of this block
				let tile_index: Arc<TileIndex> = self.get_block_tile_index(&block).await?;
				log::trace!("tile_index.len() {}", tile_index.len());

				let tile_ranges: Vec<(TileCoord, ByteRange)> = tile_index
					.iter()
					.enumerate()
					.filter_map(|(index, range)| {
						let coord = tiles_bbox_block.coord_at_index(index as u64).ok()?;
						if tiles_bbox_used.contains(&coord) && range.length > 0 {
							Some((coord, *range))
						} else {
							None
						}
					})
					.collect();

				Ok(coalesce_into_chunks(tile_ranges))
			}
		});

		let chunks: Vec<Result<Vec<Chunk>>> = stream.collect().await;

		let chunks: Vec<Chunk> = chunks
			.into_iter()
			.collect::<Result<Vec<Vec<Chunk>>>>()?
			.into_iter()
			.flatten()
			.collect();
		Ok(chunks)
	}
}

#[async_trait]
/// [`TileSource`] implementation — provides `container_name`, `parameters`, `tilejson`,
/// on-the-fly `override_compression`, single-tile fetch via `get_tile`, and bbox streaming via
/// `get_tile_stream` (with internal read coalescing).
impl TileSource for VersaTilesReader {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("versatiles", self.reader.get_name())
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	/// Fetch a single tile by XYZ coordinate.
	///
	/// Computes the corresponding **block coordinate** (z, x>>8, y>>8), verifies membership
	/// within the block's bbox, looks up the tile's byte range from the cached index, and reads it.
	/// Returns `Ok(None)` for empty ranges or missing blocks.
	#[context("fetching tile {:?} from '{}'", coord, self.reader.get_name())]
	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		// Calculate block coordinate
		let block_coord = TileCoord::new(coord.level, coord.x.shr(8), coord.y.shr(8))?;

		// Get the block using the block coordinate
		let Some(block) = self.block_index.get_block(&block_coord) else {
			return Ok(None);
		};
		let block = block.clone();

		// Get the block and its bounding box
		let bbox = block.get_global_bbox();

		// Check if the tile is within the block definition
		if !bbox.contains(coord) {
			log::trace!("tile {coord:?} outside block definition");
			return Ok(None);
		}

		// Get the tile ID
		let tile_id = usize::try_from(bbox.index_of(coord)?)?;

		// Retrieve the tile index from cache or read from the reader
		let tile_index: Arc<TileIndex> = self.get_block_tile_index(&block).await?;
		let tile_range: ByteRange = *tile_index.get(tile_id);

		//  None if the tile range has zero length
		if tile_range.length == 0 {
			return Ok(None);
		}

		// Read the tile data from the reader
		let blob = self.reader.read_range(&tile_range).await?;
		Ok(Some(Tile::from_blob(
			blob,
			self.metadata.tile_compression,
			self.metadata.tile_format,
		)))
	}

	#[context("streaming tile sizes for bbox {:?}", bbox)]
	async fn get_tile_size_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, u32>> {
		let block_coords: Vec<TileCoord> = bbox.scaled_down(256).iter_coords().collect();

		let mut blocks: Vec<(TileBBox, TileBBox, BlockDefinition)> = Vec::new();
		for block_coord in block_coords {
			let Some(block) = self.block_index.get_block(&block_coord) else {
				continue;
			};
			let block_bbox = *block.get_global_bbox();
			let mut used_bbox = bbox;
			used_bbox.intersect_with(&block_bbox)?;
			blocks.push((block_bbox, used_bbox, block.clone()));
		}

		let reader = Arc::clone(&self.reader);

		Ok(TileStream::from_stream(
			futures::stream::iter(blocks)
				.then(move |(block_bbox, used_bbox, block)| {
					let reader = Arc::clone(&reader);
					async move {
						let blob = match reader.read_range(block.get_index_range()).await {
							Ok(blob) => blob,
							Err(e) => {
								log::error!("failed to read block index range {:?}: {e}", block.get_index_range());
								return futures::stream::iter(Vec::new());
							}
						};
						let tile_index = match TileIndex::from_brotli_blob(&blob) {
							Ok(idx) => idx,
							Err(e) => {
								log::error!("failed to decompress tile index: {e}");
								return futures::stream::iter(Vec::new());
							}
						};

						let entries: Vec<(TileCoord, u32)> = tile_index
							.iter()
							.enumerate()
							.filter_map(|(index, range)| {
								if range.length == 0 {
									return None;
								}
								let coord = block_bbox.coord_at_index(index as u64).ok()?;
								if used_bbox.contains(&coord) {
									Some((coord, u32::try_from(range.length).ok()?))
								} else {
									None
								}
							})
							.collect();

						futures::stream::iter(entries)
					}
				})
				.flatten()
				.boxed(),
		))
	}

	#[context("streaming tiles for bbox {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::debug!("get_tile_stream {bbox:?}");
		let chunks = self.get_chunks(bbox).await?;
		Ok(stream_from_chunks(
			chunks,
			Arc::clone(&self.reader),
			self.metadata.tile_compression,
			self.metadata.tile_format,
		))
	}

	// deep probe of container meta
	#[cfg(feature = "cli")]
	#[context("probing versatiles container metadata")]
	async fn probe_container(&self, print: &PrettyPrint) -> Result<()> {
		print.add_key_value("meta size", &self.header.meta_range.length).await;
		print.add_key_value("block count", &self.block_index.len()).await;

		print
			.add_key_value("sum of block index sizes", &self.get_index_size())
			.await;
		print
			.add_key_value("sum of block tiles sizes", &self.get_tiles_size())
			.await;

		Ok(())
	}

	// deep probe of container tiles
	#[cfg(feature = "cli")]
	#[context("probing versatiles tiles (scan & stats)")]
	async fn probe_tiles(&self, print: &PrettyPrint) -> Result<()> {
		#[derive(Debug)]
		#[allow(dead_code)]
		struct Entry {
			size: u64,
			x: u32,
			y: u32,
			z: u8,
		}

		let mut biggest_tiles: Vec<Entry> = Vec::new();
		let mut min_size: u64 = 0;
		let mut size_sum: u64 = 0;
		let mut tile_count: u64 = 0;

		let progress = self
			.runtime
			.create_progress("scanning blocks", self.block_index.len() as u64);

		for block in self.block_index.iter() {
			let tile_index = self.get_block_tile_index(block).await?;
			for (index, tile_range) in tile_index.iter().enumerate() {
				let size = tile_range.length;

				tile_count += 1;
				size_sum += size;

				if size < min_size {
					continue;
				}

				let bbox = block.get_global_bbox();
				let coord = bbox.coord_at_index(index as u64)?;

				let pos = biggest_tiles
					.binary_search_by(|e| e.size.cmp(&size).reverse())
					.unwrap_or_else(|p| p);
				biggest_tiles.insert(
					pos,
					Entry {
						size,
						x: coord.x,
						y: coord.y,
						z: coord.level,
					},
				);
				if biggest_tiles.len() > 10 {
					biggest_tiles.pop();
				}
				min_size = biggest_tiles.last().expect("biggest_tiles is non-empty").size;
			}
			progress.inc(1);
		}
		progress.finish();

		print
			.add_key_value("average tile size", &size_sum.div_euclid(tile_count))
			.await;

		for (index, entry) in biggest_tiles.iter().enumerate() {
			print
				.add_key_value(&format!("#{} biggest tile", index + 1), entry)
				.await;
		}

		Ok(())
	}
}

// Implement Debug for TilesReader
impl Debug for VersaTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("VersaTilesReader")
			.field("parameters", &self.metadata())
			.finish()
	}
}

impl PartialEq for VersaTilesReader {
	fn eq(&self, other: &Self) -> bool {
		self.tilejson == other.tilejson
			&& self.metadata == other.metadata
			&& self.get_tiles_size() == other.get_tiles_size()
	}
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
	use super::*;
	use crate::{MOCK_BYTES_PBF, MockReader, TilesRuntime, TilesWriter, VersaTilesWriter, make_test_file};
	use assert_fs::NamedTempFile;
	use versatiles_core::{Blob, TileBBoxPyramid, TileCompression, TileFormat, assert_wildcard, io::DataWriterBlob};

	// Helper to quickly create a test reader and bbox
	async fn mk_reader() -> Result<(NamedTempFile, VersaTilesReader)> {
		let temp_file = make_test_file(TileFormat::MVT, TileCompression::Gzip, 4, "versatiles").await?;
		let runtime = TilesRuntime::default();
		let reader = VersaTilesReader::open_path(&temp_file, runtime).await?;
		Ok((temp_file, reader))
	}

	#[tokio::test]
	async fn reader() -> Result<()> {
		let (_, reader) = mk_reader().await?;

		assert_eq!(
			format!("{reader:?}"),
			"VersaTilesReader { parameters: TileSourceMetadata { bbox_pyramid: [0: [0,0,0,0] (1x1), 1: [0,0,1,1] (2x2), 2: [0,0,3,3] (4x4), 3: [0,0,7,7] (8x8), 4: [0,0,15,15] (16x16)], tile_compression: Gzip, tile_format: MVT, traversal: Traversal(AnyOrder,1..256) } }"
		);
		assert_wildcard!(
			reader.source_type().to_string(),
			"container 'versatiles' ('*.versatiles')"
		);
		assert_eq!(
			reader.tilejson().as_string(),
			"{\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
		);
		assert_eq!(
			format!("{:?}", reader.metadata()),
			"TileSourceMetadata { bbox_pyramid: [0: [0,0,0,0] (1x1), 1: [0,0,1,1] (2x2), 2: [0,0,3,3] (4x4), 3: [0,0,7,7] (8x8), 4: [0,0,15,15] (16x16)], tile_compression: Gzip, tile_format: MVT, traversal: Traversal(AnyOrder,1..256) }"
		);
		assert_eq!(reader.metadata().tile_compression, TileCompression::Gzip);
		assert_eq!(reader.metadata().tile_format, TileFormat::MVT);

		let blob = reader
			.get_tile(&TileCoord::new(4, 15, 1)?)
			.await?
			.unwrap()
			.into_blob(TileCompression::Uncompressed)?;
		assert_eq!(blob.as_slice(), MOCK_BYTES_PBF);

		let sizes = reader
			.get_tile_stream(TileBBox::new_full(4)?)
			.await?
			.map_parallel_try(|_coord, mut tile| Ok(tile.as_blob(TileCompression::Gzip)?.len()))
			.unwrap_results();
		let sizes: Vec<(TileCoord, u64)> = sizes.to_vec().await;
		assert_eq!(sizes.len(), 256);
		for (_, size) in sizes {
			assert_eq!(size, 77);
		}

		Ok(())
	}

	#[tokio::test]
	async fn tile_stream_matches_individual_blob_reads() -> Result<()> {
		let (_, reader) = mk_reader().await?;
		let bbox = TileBBox::new_full(4)?;
		let stream = reader.get_tile_stream(bbox).await?;
		let mut all: Vec<(TileCoord, Blob)> = stream
			.map_parallel_try(|_coord, tile| tile.into_blob(TileCompression::Uncompressed))
			.unwrap_results()
			.to_vec()
			.await;
		all.sort_by_key(|(c, _)| (c.y, c.x));
		assert_eq!(all.len(), bbox.count_tiles() as usize);

		// Spot check a few coordinates (corners + center)
		let probes = [
			TileCoord::new(4, 0, 0)?,
			TileCoord::new(4, 15, 0)?,
			TileCoord::new(4, 0, 15)?,
			TileCoord::new(4, 15, 15)?,
			TileCoord::new(4, 7, 8)?,
		];
		for coord in probes {
			let from_stream = all
				.iter()
				.find(|(c, _)| *c == coord)
				.map(|(_, b)| b.clone())
				.expect("present in stream");
			let from_single = reader
				.get_tile(&coord)
				.await?
				.expect("present via single read")
				.into_blob(TileCompression::Uncompressed)?;
			assert_eq!(
				from_stream.as_slice(),
				from_single.as_slice(),
				"blob mismatch at {coord:?}"
			);
		}
		Ok(())
	}

	#[tokio::test]
	async fn get_tile_out_of_range_is_none() -> Result<()> {
		let (_, reader) = mk_reader().await?;
		// level beyond available
		assert!(reader.get_tile(&TileCoord::new(5, 0, 0)?).await?.is_none());
		Ok(())
	}

	#[tokio::test]
	async fn single_tile_bbox_streams() -> Result<()> {
		let (_, reader) = mk_reader().await?;
		let one = TileBBox::from_min_and_max(4, 15, 1, 15, 1)?;
		let blobs = reader.get_tile_stream(one).await?.to_vec().await;
		assert_eq!(blobs.len(), 1);
		Ok(())
	}

	#[tokio::test]
	async fn read_your_own_dog_food() -> Result<()> {
		let mut reader1 = MockReader::new_mock(TileSourceMetadata::new(
			TileFormat::JSON,
			TileCompression::Gzip,
			TileBBoxPyramid::new_full_up_to(4),
			Traversal::ANY,
		))?;

		let runtime = TilesRuntime::default();

		let mut data_writer1 = DataWriterBlob::new()?;
		VersaTilesWriter::write_to_writer(&mut reader1, &mut data_writer1, runtime.clone()).await?;

		let data_reader1 = data_writer1.to_reader();
		let mut reader2 = VersaTilesReader::open_reader(Box::new(data_reader1), runtime.clone()).await?;

		let mut data_writer2 = DataWriterBlob::new()?;
		VersaTilesWriter::write_to_writer(&mut reader2, &mut data_writer2, runtime.clone()).await?;

		let data_reader2 = data_writer2.to_reader();
		let reader3 = VersaTilesReader::open_reader(Box::new(data_reader2), runtime).await?;

		assert_eq!(reader2, reader3);

		Ok(())
	}

	#[tokio::test]
	#[cfg(feature = "cli")]
	async fn probe() -> Result<()> {
		let (_, reader) = mk_reader().await?;

		let mut printer = PrettyPrint::new();
		reader.probe_container(&printer.get_category("container").await).await?;
		assert_eq!(
			printer.as_string().await,
			"container:\n  meta size: 58\n  block count: 5\n  sum of block index sizes: 70\n  sum of block tiles sizes: 385\n"
		);

		let mut printer = PrettyPrint::new();
		reader.probe_tiles(&printer.get_category("tiles").await).await?;
		assert_eq!(
			printer.as_string().await.get(0..67).unwrap(),
			"tiles:\n  average tile size: 77\n  #1 biggest tile: Entry { size: 77,"
		);

		Ok(())
	}

	#[tokio::test]
	async fn tile_size_stream_full_level() -> Result<()> {
		let (_, reader) = mk_reader().await?;
		let bbox = TileBBox::new_full(4)?;
		let mut sizes: Vec<(TileCoord, u32)> = reader.get_tile_size_stream(bbox).await?.to_vec().await;
		sizes.sort_by_key(|(c, _)| (c.y, c.x));

		assert_eq!(sizes.len(), 256);
		for (_, size) in &sizes {
			assert_eq!(*size, 77);
		}

		Ok(())
	}

	#[tokio::test]
	async fn tile_size_stream_sub_bbox() -> Result<()> {
		let (_, reader) = mk_reader().await?;
		let bbox = TileBBox::from_min_and_max(4, 2, 3, 5, 6)?;
		let sizes: Vec<(TileCoord, u32)> = reader.get_tile_size_stream(bbox).await?.to_vec().await;

		assert_eq!(sizes.len(), 16); // 4x4
		for (coord, size) in &sizes {
			assert!(bbox.contains(coord), "coord {coord:?} outside requested bbox");
			assert_eq!(*size, 77);
		}

		Ok(())
	}

	#[tokio::test]
	async fn tile_size_stream_single_tile() -> Result<()> {
		let (_, reader) = mk_reader().await?;
		let bbox = TileBBox::from_min_and_max(4, 15, 1, 15, 1)?;
		let sizes: Vec<(TileCoord, u32)> = reader.get_tile_size_stream(bbox).await?.to_vec().await;

		assert_eq!(sizes.len(), 1);
		assert_eq!(sizes[0].0, TileCoord::new(4, 15, 1)?);
		assert_eq!(sizes[0].1, 77);

		Ok(())
	}

	#[tokio::test]
	async fn tile_size_stream_matches_tile_stream() -> Result<()> {
		let (_, reader) = mk_reader().await?;
		let bbox = TileBBox::new_full(4)?;
		let compression = reader.metadata().tile_compression;

		let mut sizes: Vec<(TileCoord, u32)> = reader.get_tile_size_stream(bbox).await?.to_vec().await;
		sizes.sort_by_key(|(c, _)| (c.level, c.y, c.x));

		let mut blob_sizes: Vec<(TileCoord, u32)> = reader
			.get_tile_stream(bbox)
			.await?
			.map(move |_coord, tile| {
				u32::try_from(tile.into_blob(compression).expect("tile should have blob").len()).expect("size fits u32")
			})
			.to_vec()
			.await;
		blob_sizes.sort_by_key(|(c, _)| (c.level, c.y, c.x));

		assert_eq!(sizes.len(), blob_sizes.len());
		for (a, b) in sizes.iter().zip(blob_sizes.iter()) {
			assert_eq!(a.0, b.0, "coord mismatch");
			assert_eq!(a.1, b.1, "size mismatch at {:?}", a.0);
		}

		Ok(())
	}

	#[tokio::test]
	async fn tile_size_stream_out_of_range_is_empty() -> Result<()> {
		let (_, reader) = mk_reader().await?;
		let bbox = TileBBox::new_full(5)?;
		let sizes: Vec<(TileCoord, u32)> = reader.get_tile_size_stream(bbox).await?.to_vec().await;

		assert!(sizes.is_empty());

		Ok(())
	}
}
