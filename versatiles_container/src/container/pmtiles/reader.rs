//! Read tiles and metadata from a PMTiles (v3) container.
//!
//! The `PMTilesReader` parses the PMTiles v3 header and directory structure, reads the
//! embedded TileJSON metadata, and fetches tile blobs by translating XYZ tile coordinates
//! into **Hilbert indices**. It supports internal compression used by PMTiles for
//! metadata/directories (e.g., gzip) as well as the **transport compression** of the tiles
//! themselves (e.g., gzip for MVT tiles) as declared in the header.
//!
//! ## What it extracts
//! - `header`: parsed [`HeaderV3`] with offsets and compression flags
//! - `tilejson`: parsed TileJSON (from `metadata` range), merged into [`TileJSON`]
//! - `parameters`: [`TileSourceMetadata`] with `tile_format`, `tile_compression`, and a
//!   computed **bbox pyramid** inferred from the directory tree
//!
//! ## Requirements
//! - Use an **absolute** filesystem path when opening via [`open_path`].
//! - The container must be a valid PMTiles v3 file with readable header, directories, and data.
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
//!     let mut reader = PMTilesReader::open_path(path, runtime).await?;
//!
//!     // Inspect metadata
//!     let tj = reader.tilejson();
//!     println!("format={:?} compression={:?}", reader.metadata().tile_format, reader.metadata().tile_compression);
//!
//!     // Fetch a tile
//!     let coord = TileCoord::new(14, 8800, 5370)?;
//!     if let Some(mut tile) = reader.get_tile(&coord).await? {
//!         let _blob = tile.as_blob(reader.metadata().tile_compression)?;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Errors
//! Returns errors when the path is not absolute, the file cannot be read, the
//! PMTiles header/directories cannot be parsed or decompressed, or a requested tile is missing.

use super::types::{EntriesV3, HeaderV3};
use crate::{SourceType, Tile, TileSource, TileSourceMetadata, TilesRuntime, Traversal, TraversalOrder, TraversalSize};
use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::lock::Mutex;
use std::{fmt::Debug, path::Path, sync::Arc};
#[cfg(feature = "cli")]
use versatiles_core::utils::PrettyPrint;
use versatiles_core::{
	io::*,
	utils::{HilbertIndex, decompress},
	*,
};
use versatiles_derive::context;

/// Reader for PMTiles v3 containers.
///
/// Parses the header and directory blobs, merges embedded TileJSON, computes a
/// bounding-box pyramid by traversing directory entries, and exposes tiles via
/// the [`TileSource`] interface.
#[derive(Debug)]
pub struct PMTilesReader {
	/// Underlying byte source used to read header, directories, and tile data.
	pub data_reader: DataReader,
	/// Parsed PMTiles v3 header with byte ranges, counts, and compression flags.
	pub header: HeaderV3,
	/// Compression algorithm used for internal metadata/directories (e.g., gzip).
	pub internal_compression: TileCompression,
	/// Raw (compressed) concatenated blob of all leaf directories.
	pub leaves_bytes: Blob,
	/// Decompression cache mapping leaf directory byte ranges to parsed entries.
	pub leaves_cache: Mutex<LimitedCache<ByteRange, Arc<EntriesV3>>>,
	/// Merged TileJSON metadata extracted from the PMTiles `metadata` range.
	pub tilejson: TileJSON,
	/// Runtime parameters (tile format, compression, bbox pyramid) advertised by this reader.
	pub metadata: TileSourceMetadata,
	/// Uncompressed root directory blob.
	pub root_bytes_uncompressed: Blob,
	/// Parsed entries of the root directory (shared across queries).
	pub root_entries: Arc<EntriesV3>,
}

impl PMTilesReader {
	/// Open a PMTiles container from an **absolute** filesystem path.
	///
	/// Validates and opens a `DataReaderFile`, then delegates to [`PMTilesReader::open_reader`].
	///
	/// # Errors
	/// Returns an error if the file cannot be opened.
	#[context("opening PMTiles at '{}'", path.display())]
	pub async fn open_path(path: &Path, runtime: TilesRuntime) -> Result<PMTilesReader> {
		PMTilesReader::open_reader(DataReaderFile::open(path)?, runtime).await
	}

	/// Open a PMTiles container from an existing [`DataReader`].
	///
	/// Reads the v3 header, decompresses and parses the metadata (`TileJSON`) and
	/// root directory, prepares leaf directory bytes, computes the bbox pyramid, and
	/// initializes caches for fast lookups.
	///
	/// # Errors
	/// Returns an error if reading or decompression fails, or if the header/dirs are invalid.
	#[context("opening PMTiles from reader")]
	pub async fn open_reader(data_reader: DataReader, runtime: TilesRuntime) -> Result<PMTilesReader>
	where
		Self: Sized,
	{
		log::debug!("Opening PMTilesReader for {}", data_reader.get_name());

		let header = HeaderV3::deserialize(&data_reader.read_range(&ByteRange::new(0, HeaderV3::len())).await?)?;
		log::trace!("Header: {:?}", header);

		let internal_compression = header.internal_compression.as_value()?;
		log::trace!("Internal compression: {:?}", internal_compression);

		let meta = data_reader.read_range(&header.metadata).await?;
		let meta = decompress(meta, internal_compression)?;
		let tilejson = TileJSON::try_from_blob_or_default(&meta);
		log::trace!("TileJSON: {:?}", tilejson);

		let root_bytes = data_reader.read_range(&header.root_dir).await?;
		log::trace!("Root directory bytes length: {}", root_bytes.len());

		let root_bytes_uncompressed = decompress(root_bytes, internal_compression)?;
		log::trace!(
			"Root directory bytes uncompressed length: {}",
			root_bytes_uncompressed.len()
		);

		let leaves_bytes = data_reader.read_range(&header.leaf_dirs).await?;
		log::trace!("Leaf directories bytes length: {}", leaves_bytes.len());

		let bbox_pyramid = calc_bbox_pyramid(
			&root_bytes_uncompressed,
			&leaves_bytes,
			internal_compression,
			runtime.clone(),
		)?;
		log::trace!("Bounding box pyramid: {:?}", bbox_pyramid);

		let metadata = TileSourceMetadata::new(
			header.tile_type.as_value()?,
			header.tile_compression.as_value()?,
			bbox_pyramid,
			Traversal {
				order: TraversalOrder::PMTiles,
				size: TraversalSize::new_default(),
			},
		);
		log::trace!("Reader parameters: {:?}", metadata);

		let root_entries = Arc::new(EntriesV3::from_blob(&root_bytes_uncompressed)?);

		Ok(PMTilesReader {
			data_reader,
			header,
			internal_compression,
			leaves_bytes,
			leaves_cache: Mutex::new(LimitedCache::with_maximum_size(100_000_000)),
			tilejson,
			metadata,
			root_bytes_uncompressed,
			root_entries,
		})
	}

	/// Decode and return the root directory entries (`EntriesV3`).
	#[context("reading PMTiles root entries")]
	pub fn get_tile_entries(&self) -> Result<EntriesV3> {
		EntriesV3::from_blob(&self.root_bytes_uncompressed)
	}
}

/// Build the per‑zoom bounding box pyramid by traversing PMTiles directory entries.
///
/// Walks the root and leaf directory blobs, following entry ranges. For `run_length`
/// entries, expands the run into individual tiles via Hilbert indices; for directory
/// entries, decompresses and recurses. Returns the accumulated [`TileBBoxPyramid`].
///
/// ### Parameters
/// - `root_bytes_uncompressed`: uncompressed root directory bytes.
/// - `leaves_bytes`: concatenated (compressed) leaf directory bytes as a single blob.
/// - `compression`: compression algorithm used for directory blobs.
///
/// ### Errors
/// Returns an error when directory blobs cannot be parsed or decompressed.
#[context("building bbox pyramid from PMTiles directories")]
fn calc_bbox_pyramid(
	root_bytes_uncompressed: &Blob,
	leaves_bytes: &Blob,
	compression: TileCompression,
	runtime: TilesRuntime,
) -> Result<TileBBoxPyramid> {
	let mut bbox_pyramid = TileBBoxPyramid::new_empty();

	parse_directories(
		&mut bbox_pyramid,
		root_bytes_uncompressed,
		leaves_bytes,
		compression,
		Some(runtime),
	)?;

	fn parse_directories(
		bbox_pyramid: &mut TileBBoxPyramid,
		dir: &Blob,
		leaves_bytes: &Blob,
		compression: TileCompression,
		root_runtime: Option<TilesRuntime>,
	) -> Result<u64> {
		log::trace!("parse_directories");

		let entries = EntriesV3::from_blob(dir)?;
		let entries = entries.iter().collect::<Vec<_>>();
		let progress = root_runtime
			.as_ref()
			.map(|runtime| runtime.create_progress("Parsing PMTiles directories", entries.len() as u64));

		let mut total_entries = 0;
		for entry in entries.iter() {
			if let Some(progress) = &progress {
				progress.inc(1);
			}

			if entry.range.length > 0 {
				if entry.run_length > 0 {
					for i in 0..entry.run_length as u64 {
						let coord = TileCoord::from_hilbert_index(i + entry.tile_id)?;
						bbox_pyramid.include_coord(&coord);
					}
					total_entries += entry.run_length as u64;
				} else {
					let range = entry.range;
					let mut blob = leaves_bytes.read_range(&range)?;
					blob = decompress(blob, compression)?;
					total_entries += parse_directories(bbox_pyramid, &blob, leaves_bytes, compression, None)?;
				}
			}
		}

		if let Some(progress) = progress {
			progress.finish();
			log::trace!("Found {} PMTiles entries", total_entries);
		}

		Ok(total_entries)
	}

	Ok(bbox_pyramid)
}

#[async_trait]
impl TileSource for PMTilesReader {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("pmtiles", self.data_reader.get_name())
	}

	/// Returns the current reader parameters (tile format, compression, bbox pyramid).
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	/// Returns the parsed and merged TileJSON metadata.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	/// Fetch a tile by XYZ coordinate.
	///
	/// Converts the coordinate to a **Hilbert tile ID**, then traverses up to three levels
	/// of PMTiles directories to locate the tile. Leaf directories are cached to avoid
	/// repeated decompression. Returns `Ok(None)` if the tile does not exist.
	#[context("fetching tile {:?} from PMTiles", coord)]
	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		// Log the requested tile coordinates for debugging purposes
		log::trace!("get_tile {:?}", coord);

		// Convert the tile coordinates into a unique tile ID
		let tile_id: u64 = coord.get_hilbert_index()?;
		// Start with the root directory entries
		let mut entries = self.root_entries.clone();

		// Iterate through the directory depth (up to 3 levels)
		for _depth in 0..3 {
			// Find the entry corresponding to the requested tile ID
			let entry = entries.find_tile(tile_id);

			// If the entry is not found, return None
			let entry = if let Some(entry) = entry {
				entry
			} else {
				return Ok(None);
			};

			// Check if the entry has a valid range
			if entry.range.length > 0 {
				// If the entry represents a run of tiles, directly fetch the tile data
				if entry.run_length > 0 {
					return Ok(Some(Tile::from_blob(
						self
							.data_reader
							.read_range(&entry.range.shifted_forward(self.header.tile_data.offset))
							.await?,
						self.metadata.tile_compression,
						self.metadata.tile_format,
					)));
				} else {
					// Otherwise, fetch the directory bytes for the next level
					let range = entry.range;
					let mut cache = self.leaves_cache.lock().await;
					// Use the cache to avoid redundant decompression and reading
					entries = cache.get_or_set(&range, || {
						let mut blob = self.leaves_bytes.read_range(&range)?;
						// Decompress the directory bytes
						blob = decompress(blob, self.internal_compression)?;
						let entries = EntriesV3::from_blob(&blob)?;
						Ok(Arc::new(entries))
					})?;
				}
			} else {
				// If the range is invalid, return None
				return Ok(None);
			}
		}

		// If the tile data is not found after traversing all levels, return an error
		bail!("not found")
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		self.stream_individual_tiles(bbox).await
	}

	// deep probe of container meta
	#[cfg(feature = "cli")]
	/// Adds PMTiles‑specific container metadata (the v3 header) to the CLI probe output.
	///
	/// Printed under the `"header"` key for human‑readable inspection.
	#[context("probing PMTiles container metadata")]
	async fn probe_container(&self, print: &PrettyPrint) -> Result<()> {
		print.add_key_value("header", &self.header).await;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use lazy_static::lazy_static;
	use std::{env::current_dir, path::PathBuf};
	use versatiles_core::assert_wildcard;

	lazy_static! {
		static ref PATH: PathBuf = current_dir().unwrap().join("../testdata/berlin.pmtiles");
	}

	#[tokio::test]
	async fn reader() -> Result<()> {
		let reader = PMTilesReader::open_path(&PATH, TilesRuntime::default()).await?;

		assert_wildcard!(
			reader.source_type().to_string(),
			"container 'pmtiles' ('*testdata?berlin.pmtiles')"
		);

		assert_eq!(
			format!("{:?}", reader.header),
			"HeaderV3 { root_dir: ByteRange[127,2271], metadata: ByteRange[2398,592], leaf_dirs: ByteRange[2990,0], tile_data: ByteRange[2990,25869006], addressed_tiles_count: 878, tile_entries_count: 878, tile_contents_count: 876, clustered: true, internal_compression: Gzip, tile_compression: Gzip, tile_type: MVT, min_zoom: 0, max_zoom: 14, min_lon_e7: 130828300, min_lat_e7: 523344600, max_lon_e7: 137622450, max_lat_e7: 526783000, center_zoom: 7, center_lon_e7: 134225380, center_lat_e7: 525063800 }"
		);

		assert_wildcard!(
			reader.tilejson().as_string(),
			"{\"author\":\"OpenStreetMap contributors, Geofabrik GmbH\",*,\"version\":\"3.0\"}"
		);

		assert_wildcard!(
			format!("{:?}", reader.metadata()),
			"TileSourceMetadata { bbox_pyramid: [0: [0,0,0,0] (1x1), 1: [1,0,1,0] (1x1), 2: [2,1,2,1] (1x1), 3: [4,2,4,2] (1x1), 4: [8,5,8,5] (1x1), 5: [17,10,17,10] (1x1), 6: [34,20,34,21] (1x2), 7: [68,41,68,42] (1x2), 8: [137,83,137,84] (1x2), 9: [274,167,275,168] (2x2), 10: [549,335,551,336] (3x2), 11: [1098,670,1102,673] (5x4), 12: [2196,1340,2204,1346] (9x7), 13: [4393,2680,4409,2693] (17x14), 14: [8787,5361,8818,5387] (32x27)], tile_compression: Gzip, tile_format: MVT, traversal: Traversal(PMTiles,full) }"
		);

		assert_eq!(
			reader
				.get_tile(&TileCoord::new(0, 0, 0)?)
				.await?
				.unwrap()
				.as_blob(reader.metadata.tile_compression)?
				.len(),
			20
		);

		assert_eq!(
			reader
				.get_tile(&TileCoord::new(14, 8800, 5370)?)
				.await?
				.unwrap()
				.as_blob(reader.metadata.tile_compression)?
				.len(),
			100391
		);

		assert!(reader.get_tile(&TileCoord::new(16, 0, 0)?).await?.is_none());

		Ok(())
	}
}
