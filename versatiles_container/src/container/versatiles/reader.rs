#![allow(dead_code)]

//! Read tiles and metadata from a `.versatiles` container.
//!
//! The `VersaTilesReader` parses the container header, decompresses the **block index**,
//! reads embedded TileJSON metadata, and exposes tiles via [`TilesReaderTrait`]. The
//! file format organizes data into fixed **256×256 tile blocks**; each block stores
//! a Brotli-compressed tile index (byte ranges), followed by a contiguous region of
//! tile blobs. This reader lazily caches decoded tile indices for fast random access.
//!
//! ## Extracted artifacts
//! - `tilejson`: parsed TileJSON from the `meta_range` (if present)
//! - `parameters`: [`TilesReaderParameters`] with `tile_format`, `tile_compression`, and a
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
//!     let path = Path::new("./data/world.versatiles");
//!     let mut reader = VersaTilesReader::open_path(&path).await?;
//!
//!     // Inspect parameters & TileJSON
//!     let params = reader.parameters();
//!     let tj = reader.tilejson();
//!     println!("format={:?} compression={:?}", params.tile_format, params.tile_compression);
//!
//!     // Fetch one tile
//!     if let Some(mut tile) = reader.get_tile(&TileCoord::new(15, 1, 4)?).await? {
//!         let _blob = tile.as_blob(params.tile_compression)?;
//!     }
//!
//!     // Stream a bbox (coalesces reads per block for fewer I/O calls)
//!     let bbox = params.bbox_pyramid.get_level_bbox(4).clone();
//!     let mut stream = reader.get_tile_stream(bbox).await?;
//!     while let Some((coord, mut tile)) = stream.next().await {
//!         let _size = tile.as_blob(params.tile_compression)?.len();
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
use crate::{Tile, TilesReaderTrait};
use anyhow::Result;
use async_trait::async_trait;
use futures::{lock::Mutex, stream::StreamExt};
use std::{fmt::Debug, ops::Shr, path::Path, sync::Arc};
#[cfg(feature = "cli")]
use versatiles_core::utils::PrettyPrint;
use versatiles_core::{io::*, utils::decompress, *};
use versatiles_derive::context;

/// Reader for `.versatiles` containers.
///
/// Decompresses and parses the block index, merges embedded TileJSON, computes a
/// per-zoom bounding-box pyramid, and serves tiles via lazy index lookups. Tile
/// indices are cached (least-recently-used) to accelerate repeated random access.
pub struct VersaTilesReader {
	block_index: BlockIndex,
	header: FileHeader,
	parameters: TilesReaderParameters,
	reader: DataReader,
	tile_index_cache: Mutex<LimitedCache<TileCoord, Arc<TileIndex>>>,
	tilejson: TileJSON,
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
	pub async fn open_path(path: &Path) -> Result<VersaTilesReader> {
		VersaTilesReader::open_reader(DataReaderFile::open(path)?).await
	}

	/// Open a `.versatiles` container from an existing [`DataReader`].
	///
	/// Reads the header, loads and (if present) decompresses the TileJSON metadata, then
	/// reads and decompresses the **block index** (Brotli). Finally, computes the bbox pyramid
	/// from the block index and initializes the tile-index cache.
	///
	/// # Errors
	/// Returns an error if header/metadata/index reads or decompressions fail.
	#[context("Failed to open versatiles reader")]
	pub async fn open_reader(mut reader: DataReader) -> Result<VersaTilesReader> {
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

		let block_index = BlockIndex::from_brotli_blob(
			reader
				.read_range(&header.blocks_range)
				.await
				.context("Failed reading the block index")?,
		)
		.context("Failed decompressing the block index")?;

		let bbox_pyramid = block_index.get_bbox_pyramid();
		let parameters = TilesReaderParameters::new(header.tile_format, header.compression, bbox_pyramid);

		Ok(VersaTilesReader {
			block_index,
			header,
			parameters,
			reader,
			tile_index_cache: Mutex::new(LimitedCache::with_maximum_size(100_000_000)),
			tilejson,
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
			let mut tile_index = TileIndex::from_brotli_blob(blob)?;
			tile_index.add_offset(block.get_tiles_range().offset);

			assert_eq!(tile_index.len(), block.count_tiles() as usize);

			cache.add(*block_coord, Arc::new(tile_index))
		})
	}

	/// Sum of all block index byte lengths.
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
	async fn get_chunks(&self, bbox: TileBBox) -> Vec<Chunk> {
		const MAX_CHUNK_SIZE: u64 = 64 * 1024 * 1024;
		const MAX_CHUNK_GAP: u64 = 32 * 1024;

		let block_coords: Vec<TileCoord> = bbox.scaled_down(256).iter_coords().collect();

		let stream = futures::stream::iter(block_coords).then(|block_coord: TileCoord| {
			async move {
				// Get the block using the block coordinate
				let block_option = self.block_index.get_block(&block_coord);
				if block_option.is_none() {
					return Vec::new();
				}

				// Get the block
				let block: BlockDefinition = block_option.unwrap().to_owned();
				log::trace!("block {block:?}");

				// Get the bounding box of all tiles defined in this block
				let tiles_bbox_block = block.get_global_bbox();
				log::trace!("tiles_bbox_block {tiles_bbox_block:?}");

				// Get the bounding box of all tiles defined in this block
				let mut tiles_bbox_used: TileBBox = bbox;
				tiles_bbox_used.intersect_with(tiles_bbox_block).unwrap();
				log::trace!("tiles_bbox_used {tiles_bbox_used:?}");

				assert_eq!(bbox.level, tiles_bbox_block.level);
				assert_eq!(bbox.level, tiles_bbox_used.level);

				// Get the tile index of this block
				let tile_index: Arc<TileIndex> = self.get_block_tile_index(&block).await.unwrap();
				log::trace!("tile_index.len() {}", tile_index.len());

				// let tile_range: &ByteRange = tile_index.get(tile_id);
				let mut tile_ranges: Vec<(TileCoord, ByteRange)> = tile_index
					.iter()
					.enumerate()
					.map(|(index, range)| (tiles_bbox_block.coord_at_index(index as u64).unwrap(), *range))
					.filter(|(coord, range)| tiles_bbox_used.contains(coord) && (range.length > 0))
					.collect();

				if tile_ranges.is_empty() {
					return Vec::new();
				}

				tile_ranges.sort_by_key(|e| e.1.offset);

				let mut chunks: Vec<Chunk> = Vec::new();
				let mut chunk = Chunk::new(tile_ranges[0].1.offset);

				for entry in tile_ranges {
					let chunk_start = chunk.range.offset;
					let chunk_end = chunk.range.offset + chunk.range.length;

					let tile_start = entry.1.offset;
					let tile_end = entry.1.offset + entry.1.length;

					if (chunk_start + MAX_CHUNK_SIZE > tile_end) && (chunk_end + MAX_CHUNK_GAP > tile_start) {
						// chunk size is still inside the limits
						chunk.push(entry);
					} else {
						// chunk becomes to big, create a new one
						chunks.push(chunk);
						chunk = Chunk::new(entry.1.offset);
						chunk.push(entry);
					}
				}

				if chunk.len() > 0 {
					chunks.push(chunk);
				}

				chunks
			}
		});

		let chunks: Vec<Vec<Chunk>> = stream.collect().await;

		let chunks: Vec<Chunk> = chunks.into_iter().flatten().collect();
		chunks
	}
}

unsafe impl Send for VersaTilesReader {}
unsafe impl Sync for VersaTilesReader {}

// Internal helper to group tile reads: collects (coord, range) pairs that can be served
// from a single large read. `range` tracks the combined byte span in the container.
#[derive(Debug)]
struct Chunk {
	tiles: Vec<(TileCoord, ByteRange)>,
	range: ByteRange,
}

impl Chunk {
	fn new(start: u64) -> Self {
		Self {
			tiles: Vec::new(),
			range: ByteRange::new(start, 0),
		}
	}
	fn push(&mut self, entry: (TileCoord, ByteRange)) {
		self.tiles.push(entry);
		if entry.1.offset < self.range.offset {
			panic!()
		};
		self.range.length = self
			.range
			.length
			.max(entry.1.offset + entry.1.length - self.range.offset)
	}
	fn len(&self) -> usize {
		self.tiles.len()
	}
}

#[async_trait]
/// [`TilesReaderTrait`] implementation — provides `container_name`, `parameters`, `tilejson`,
/// on-the-fly `override_compression`, single-tile fetch via `get_tile`, and bbox streaming via
/// `get_tile_stream` (with internal read coalescing).
impl TilesReaderTrait for VersaTilesReader {
	/// Gets the container name.
	fn container_name(&self) -> &str {
		"versatiles"
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.parameters.tile_compression = tile_compression;
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
		let block = self.block_index.get_block(&block_coord);

		if block.is_none() {
			return Ok(None);
		}
		let block = block.unwrap().clone();

		// Get the block and its bounding box
		let bbox = block.get_global_bbox();

		// Check if the tile is within the block definition
		if !bbox.contains(coord) {
			log::trace!("tile {coord:?} outside block definition");
			return Ok(None);
		}

		// Get the tile ID
		let tile_id = bbox.index_of(coord).unwrap() as usize;

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
			self.parameters.tile_compression,
			self.parameters.tile_format,
		)))
	}

	#[context("streaming tiles for bbox {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_tile_stream {:?}", bbox);
		let chunks = self.get_chunks(bbox).await;
		Ok(TileStream::from_stream(
			futures::stream::iter(chunks)
				.then(move |chunk| async move {
					let big_blob = self.reader.read_range(&chunk.range).await.unwrap();

					let entries: Vec<(TileCoord, Tile)> = chunk
						.tiles
						.into_iter()
						.map(|(coord, range)| {
							assert!(bbox.contains(&coord), "outer_bbox {bbox:?} does not contain {coord:?}");

							let start = range.offset - chunk.range.offset;
							let end = start + range.length;
							let tile_range = (start as usize)..(end as usize);

							let blob = Blob::from(big_blob.get_range(tile_range));
							let tile = Tile::from_blob(blob, self.parameters.tile_compression, self.parameters.tile_format);

							(coord, tile)
						})
						.collect();

					futures::stream::iter(entries)
				})
				.flatten()
				.boxed(),
		))
	}

	// Get the name of the reader
	fn source_name(&self) -> &str {
		self.reader.get_name()
	}

	// deep probe of container meta
	#[cfg(feature = "cli")]
	#[context("probing versatiles container metadata")]
	async fn probe_container(&mut self, print: &PrettyPrint) -> Result<()> {
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
	async fn probe_tiles(&mut self, print: &PrettyPrint) -> Result<()> {
		use versatiles_core::progress::get_progress_bar;

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

		let block_index = self.block_index.clone();
		let progress = get_progress_bar("scanning blocks", block_index.len() as u64);

		for block in block_index.iter() {
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

				biggest_tiles.push(Entry {
					size,
					x: coord.x,
					y: coord.y,
					z: coord.level,
				});
				biggest_tiles.sort_by(|a, b| b.size.cmp(&a.size));
				while biggest_tiles.len() > 10 {
					biggest_tiles.pop();
				}
				min_size = biggest_tiles.last().unwrap().size;
			}
			progress.inc(1);
		}
		progress.remove();

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
			.field("parameters", &self.parameters())
			.finish()
	}
}

impl PartialEq for VersaTilesReader {
	fn eq(&self, other: &Self) -> bool {
		self.tilejson == other.tilejson
			&& self.parameters == other.parameters
			&& self.get_tiles_size() == other.get_tiles_size()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MOCK_BYTES_PBF, MockTilesReader, ProcessingConfig, TilesWriterTrait, VersaTilesWriter, make_test_file};
	use assert_fs::NamedTempFile;
	use versatiles_core::{assert_wildcard, io::DataWriterBlob};

	// Helper to quickly create a test reader and bbox
	async fn mk_reader() -> Result<(NamedTempFile, VersaTilesReader)> {
		let temp_file = make_test_file(TileFormat::MVT, TileCompression::Gzip, 4, "versatiles").await?;
		let reader = VersaTilesReader::open_path(&temp_file).await?;
		Ok((temp_file, reader))
	}

	#[tokio::test]
	async fn reader() -> Result<()> {
		let (_, reader) = mk_reader().await?;

		assert_eq!(
			format!("{reader:?}"),
			"VersaTilesReader { parameters: TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1x1), 1: [0,0,1,1] (2x2), 2: [0,0,3,3] (4x4), 3: [0,0,7,7] (8x8), 4: [0,0,15,15] (16x16)], tile_compression: Gzip, tile_format: MVT } }"
		);
		assert_eq!(reader.container_name(), "versatiles");
		assert_wildcard!(reader.source_name(), "*.versatiles");
		assert_eq!(
			reader.tilejson().as_string(),
			"{\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
		);
		assert_eq!(
			format!("{:?}", reader.parameters()),
			"TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1x1), 1: [0,0,1,1] (2x2), 2: [0,0,3,3] (4x4), 3: [0,0,7,7] (8x8), 4: [0,0,15,15] (16x16)], tile_compression: Gzip, tile_format: MVT }"
		);
		assert_eq!(reader.parameters().tile_compression, TileCompression::Gzip);
		assert_eq!(reader.parameters().tile_format, TileFormat::MVT);

		let blob = reader
			.get_tile(&TileCoord::new(4, 15, 1)?)
			.await?
			.unwrap()
			.into_blob(TileCompression::Uncompressed)?;
		assert_eq!(blob.as_slice(), MOCK_BYTES_PBF);

		let sizes = reader
			.get_tile_stream(TileBBox::new_full(4)?)
			.await?
			.map_item_parallel(|mut tile| Ok(tile.as_blob(TileCompression::Gzip)?.len()));
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
			.map_item_parallel(|tile| tile.into_blob(TileCompression::Uncompressed))
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
		let mut reader1 = MockTilesReader::new_mock(TilesReaderParameters::new(
			TileFormat::JSON,
			TileCompression::Gzip,
			TileBBoxPyramid::new_full(4),
		))?;

		let mut data_writer1 = DataWriterBlob::new()?;
		VersaTilesWriter::write_to_writer(&mut reader1, &mut data_writer1, ProcessingConfig::default()).await?;

		let data_reader1 = data_writer1.to_reader();
		let mut reader2 = VersaTilesReader::open_reader(Box::new(data_reader1)).await?;

		let mut data_writer2 = DataWriterBlob::new()?;
		VersaTilesWriter::write_to_writer(&mut reader2, &mut data_writer2, ProcessingConfig::default()).await?;

		let data_reader2 = data_writer2.to_reader();
		let reader3 = VersaTilesReader::open_reader(Box::new(data_reader2)).await?;

		assert_eq!(reader2, reader3);

		Ok(())
	}

	#[tokio::test]
	#[cfg(feature = "cli")]
	async fn probe() -> Result<()> {
		let (_, mut reader) = mk_reader().await?;

		let mut printer = PrettyPrint::new();
		reader.probe_container(&printer.get_category("container").await).await?;
		assert_eq!(
			printer.as_string().await,
			"container:\n  meta size: 58\n  block count: 5\n  sum of block index sizes: 70\n  sum of block tiles sizes: 385\n"
		);

		let mut printer = PrettyPrint::new();
		reader.probe_tiles(&printer.get_category("tiles").await).await?;
		assert_eq!(
			printer.as_string().await.get(0..73).unwrap(),
			"tiles:\n  average tile size: 77\n  #1 biggest tile: Entry { size: 77, x: 0,"
		);

		Ok(())
	}
}
