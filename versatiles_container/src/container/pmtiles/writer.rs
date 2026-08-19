//! Write tiles and metadata into a `PMTiles` v3 container.
//!
//! The `PMTilesWriter` produces a valid [PMTiles v3](https://github.com/protomaps/PMTiles) archive
//! from any [`TileSource`] source. It serializes tile data, compresses metadata and directories,
//! and builds a compact Hilbert-ordered directory layout.
//!
//! ## Behavior
//! - Compresses the metadata (`TileJSON`) and directory blocks with internal **gzip** compression.
//! - Stores tiles in **Hilbert order** for spatial locality.
//! - Uses `PMTiles` v3 header fields to describe data offsets and compression types.
//! - Produces a single binary blob that can be read back by [`PMTilesReader`](crate::container::pmtiles::PMTilesReader).
//!
//! ## Requirements
//! - The writer must output to a valid [`DataWriterTrait`] target (e.g. file, blob, memory).
//! - The input [`TileSource`] must provide consistent `tile_format` and `tile_compression`.
//!
//! ## Example
//! ```rust,no_run
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use std::path::Path;
//! use anyhow::{Result, ensure};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let runtime = TilesRuntime::default();
//!
//!     // Open an existing MBTiles file
//!     let path = Path::new("/absolute/path/to/berlin.mbtiles");
//!     let mut reader = MBTilesReader::open(&path, runtime.clone())?;
//!
//!     // Convert it to PMTiles format
//!     let temp_path = std::env::temp_dir().join("berlin.pmtiles");
//!     PMTilesWriter::write_to_path(&mut reader, &temp_path, runtime).await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Errors
//! Returns errors if writing, compression, or serialization fails.

use std::{
	collections::HashMap,
	hash::{DefaultHasher, Hash, Hasher},
	sync::Arc,
};

use anyhow::{Result, ensure};
use async_trait::async_trait;
use futures::lock::Mutex;
use versatiles_core::{
	compression::compress,
	io::DataWriterTrait,
	types::{Blob, ByteRange, TileCompression},
	utils::HilbertIndex,
};
use versatiles_derive::context;

use super::types::{EntriesV3, EntryV3, HeaderV3, PMTilesCompression};
use crate::{
	TileSource, TileSourceTraverseExt, TilesRuntime, TilesWriter,
	traversal::{Traversal, TraversalOrder},
};

/// Writer for `PMTiles` v3 archives.
///
/// Converts a [`TileSource`] source into a single `PMTiles` container by serializing
/// tiles, compressing metadata, generating directory entries, and writing a final header
/// with offsets and counts.
///
/// Tiles are ordered using the **Hilbert curve** to optimize spatial locality in the output.
pub struct PMTilesWriter {}

/// Writer option: accept a source that cannot supply Hilbert order, at the cost
/// of a truthful `clustered` flag. See [`PMTilesWriter::supported_options`].
const ALLOW_UNCLUSTERED: &str = "allow_unclustered";

#[async_trait]
impl TilesWriter for PMTilesWriter {
	/// See the module docs on clustering. `allow_unclustered` is the only option
	/// today; #231 adds `reorder`, which buys clustering back with a second pass.
	fn supported_options() -> &'static [&'static str] {
		&[ALLOW_UNCLUSTERED]
	}

	#[context("writing PMTiles to DataWriter")]
	/// Write tiles and metadata from a [`TileSource`] to a [`DataWriterTrait`] as a `PMTiles` archive.
	///
	/// This method:
	/// - Compresses metadata and directories with gzip (`INTERNAL_COMPRESSION`).
	/// - Orders tiles by Hilbert index to preserve spatial proximity.
	/// - Builds the `PMTiles` v3 header, directory blocks, and leaf entries.
	/// - Writes the final file in the correct binary layout (header + metadata + directories + tile data).
	///
	/// # Errors
	/// Returns an error if any I/O, compression, or serialization operation fails.
	async fn write_to_writer(
		reader: &mut dyn TileSource,
		writer: &mut dyn DataWriterTrait,
		runtime: TilesRuntime,
	) -> Result<()> {
		const INTERNAL_COMPRESSION: TileCompression = TileCompression::Gzip;

		let parameters = reader.metadata().clone();

		// A PMTiles archive is only *physically* clustered if tiles arrive in
		// Hilbert order; the directory does not depend on it, because
		// `EntriesV3::build_directory` sorts by tile id itself. So arrival order
		// decides one thing: whether `header.clustered = true` would be truthful.
		//
		// Checked before any tile is read — on a real pipeline the alternative is
		// failing minutes in. The destination file is already open by this point,
		// so a failure here still leaves an empty one behind;
		// `ContainerRegistry::write_to_path` removes it.
		let required = Traversal::new(TraversalOrder::PMTiles, 1, 64)?;
		let available = parameters.traversal();
		let clustered = available.can_translate_to(&required);
		let allow_unclustered = runtime.writer_option_bool(ALLOW_UNCLUSTERED)?.unwrap_or(false);

		ensure!(
			clustered || allow_unclustered,
			"cannot write PMTiles: this source produces tiles in {} order, and PMTiles stores them \
			 in Hilbert order — reordering while writing would mean holding the whole tileset in \
			 memory. Operations that build lower zoom levels from higher ones, such as \
			 `raster_overview`, force this order.\n\
			 Either write .versatiles or .mbtiles, which accept any order, or pass \
			 `--writer-option {ALLOW_UNCLUSTERED}=true` to write a PMTiles archive whose tile data is not \
			 physically clustered. The archive is valid and reads correctly either way, but an \
			 unclustered one needs more range requests to serve, so it is not chosen for you.",
			available.order()
		);

		// Only negotiate the order down once the source has been accepted, so a
		// clustered source still gets the chunked Hilbert traversal.
		let traversal = if clustered { required } else { Traversal::ANY };

		let entries = EntriesV3::new();

		writer.set_position(16384)?;

		let tilejson = reader.tilejson();
		let tile_pyramid = reader.tile_pyramid().await?;
		let mut header = HeaderV3::from_parameters(&parameters, tile_pyramid.as_ref(), tilejson);

		let mut metadata: Blob = tilejson.into();
		metadata = compress(metadata, &INTERNAL_COMPRESSION)?;
		header.metadata = writer.append(&metadata)?;

		let tile_data_start = writer.position()?;

		let writer_mutex = Arc::new(Mutex::new(writer));
		let entries_mutex = Arc::new(Mutex::new(entries));
		let dedup_map: Arc<Mutex<HashMap<u64, ByteRange>>> = Arc::new(Mutex::new(HashMap::new()));
		let tile_compression = *reader.metadata().tile_compression();

		reader
			.traverse_all_tiles(
				&traversal,
				|_bbox, stream| {
					let writer_mutex = Arc::clone(&writer_mutex);
					let entries_mutex = Arc::clone(&entries_mutex);
					let dedup_map = Arc::clone(&dedup_map);
					Box::pin(async move {
						// Pre-encode blobs in parallel (CPU-intensive work happens here)
						let stream = stream.map_parallel_try(move |_coord, mut tile| {
							tile.as_blob(&tile_compression)?;
							Ok(tile)
						});

						// Collect results, propagating encoding errors
						let mut tiles = Vec::new();
						for (coord, result) in stream.to_vec().await {
							tiles.push((coord, result?));
						}

						tiles
							.sort_by_key(|(coord, _)| coord.get_hilbert_index().expect("valid tile coord has hilbert index"));

						// Lock AFTER parallel work — write is cheap (blobs already encoded)
						let mut writer = writer_mutex.lock().await;
						let mut entries = entries_mutex.lock().await;
						let mut dedup = dedup_map.lock().await;
						for (coord, mut tile) in tiles {
							let id = coord.get_hilbert_index()?;
							let blob = tile.as_blob(&tile_compression)?;

							let mut hasher = DefaultHasher::new();
							blob.as_slice().hash(&mut hasher);
							let hash = hasher.finish();

							let range = if let Some(&existing) = dedup.get(&hash) {
								existing
							} else {
								let range = writer.append(blob)?.shifted_backward(tile_data_start)?;
								dedup.insert(hash, range);
								range
							};

							entries.push(EntryV3::new(id, range, 1));
						}
						Ok(())
					})
				},
				runtime.clone(),
			)
			.await?;

		let mut entries = entries_mutex.lock().await;
		let mut writer = writer_mutex.lock().await;
		let tile_contents_count = dedup_map.lock().await.len() as u64;

		let tile_data_end = writer.position()?;

		header.tile_data = ByteRange::new(tile_data_start, tile_data_end - tile_data_start);

		writer.set_position(HeaderV3::len())?;
		// `build_directory` sorts by tile id; `merge_runs` documents that it
		// requires sorted input. Today the traversal happens to deliver entries
		// sorted, so the order of these two worked by accident.
		let directory = entries.build_directory(16384 - HeaderV3::len(), INTERNAL_COMPRESSION)?;
		header.root_dir = writer.append(&directory.root_bytes)?;

		writer.set_position(tile_data_end)?;
		header.leaf_dirs = writer.append(&directory.leaves_bytes)?;

		header.clustered = clustered;
		header.internal_compression = PMTilesCompression::from_value(INTERNAL_COMPRESSION)?;
		header.addressed_tiles_count = entries.tile_count();
		header.tile_entries_count = entries.len() as u64;
		header.tile_contents_count = tile_contents_count;

		writer.write_start(&header.serialize()?)?;

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use versatiles_core::{TileBBox, TileFormat, TilePyramid, io::*};
	use versatiles_derive::context;

	use super::*;
	use crate::{
		TileSourceMetadata,
		container::{
			mock::{MockReader, MockWriter},
			pmtiles::PMTilesReader,
		},
	};

	/// A source whose order PMTiles cannot accept — what `raster_overview`
	/// produces, and the case #228 reported.
	fn depth_first_source() -> Result<MockReader> {
		MockReader::new_mock(
			TilePyramid::new_full_up_to(3),
			TileSourceMetadata::new(
				TileFormat::PNG,
				TileCompression::Uncompressed,
				Traversal::new(TraversalOrder::DepthFirst, 1, 16)?,
				None,
			),
		)
	}

	/// The header is the first `HeaderV3::len()` bytes of the archive.
	fn read_header(blob: &Blob) -> Result<HeaderV3> {
		let len = usize::try_from(HeaderV3::len())?;
		HeaderV3::deserialize(&Blob::from(&blob.as_slice()[..len]))
	}

	async fn write_to_blob(reader: &mut MockReader, runtime: TilesRuntime) -> Result<Blob> {
		let mut writer = DataWriterBlob::new()?;
		PMTilesWriter::write_to_writer(reader, &mut writer, runtime).await?;
		Ok(writer.into_blob())
	}

	#[tokio::test]
	async fn unclustered_source_is_refused_by_default() -> Result<()> {
		let err = write_to_blob(&mut depth_first_source()?, TilesRuntime::new_silent())
			.await
			.unwrap_err();
		let err = format!("{err:#}");

		assert!(err.contains("DepthFirst"), "should name the source's order: {err}");
		assert!(err.contains("raster_overview"), "should name the usual cause: {err}");
		// The failure has to name both escape hatches, so the choice is visible
		// where it is made rather than only in the documentation.
		assert!(err.contains(".versatiles"), "should offer another container: {err}");
		assert!(err.contains("allow_unclustered=true"), "should offer the opt-in: {err}");
		Ok(())
	}

	#[tokio::test]
	async fn allow_unclustered_accepts_the_source_and_says_so_in_the_header() -> Result<()> {
		let runtime = TilesRuntime::builder()
			.silent_progress(true)
			.writer_option("allow_unclustered", "true")
			.build();
		let blob = write_to_blob(&mut depth_first_source()?, runtime).await?;

		let header = read_header(&blob)?;
		assert!(
			!header.clustered,
			"an archive written out of order must not claim to be clustered"
		);
		Ok(())
	}

	#[tokio::test]
	async fn a_clustered_source_still_reports_clustered() -> Result<()> {
		// The opt-in must not make every archive claim to be unclustered: the
		// flag follows the source, not the option.
		let mut reader = MockReader::new_mock(
			TilePyramid::new_full_up_to(3),
			TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None),
		)?;
		let runtime = TilesRuntime::builder()
			.silent_progress(true)
			.writer_option("allow_unclustered", "true")
			.build();
		let blob = write_to_blob(&mut reader, runtime).await?;

		assert!(read_header(&blob)?.clustered);
		Ok(())
	}

	#[tokio::test]
	async fn allow_unclustered_false_still_refuses() -> Result<()> {
		let runtime = TilesRuntime::builder()
			.silent_progress(true)
			.writer_option("allow_unclustered", "false")
			.build();
		assert!(write_to_blob(&mut depth_first_source()?, runtime).await.is_err());
		Ok(())
	}

	#[tokio::test]
	async fn a_non_boolean_option_value_is_an_error() -> Result<()> {
		let runtime = TilesRuntime::builder()
			.silent_progress(true)
			.writer_option("allow_unclustered", "maybe")
			.build();
		let err = write_to_blob(&mut depth_first_source()?, runtime).await.unwrap_err();
		assert!(format!("{err:#}").contains("expects true or false"));
		Ok(())
	}

	#[test]
	fn the_option_is_declared_so_the_registry_accepts_it() {
		assert!(PMTilesWriter::supported_options().contains(&"allow_unclustered"));
	}

	#[context("test: PMTiles read↔write roundtrip")]
	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockReader::new_mock(
			TilePyramid::new_full_up_to(4),
			TileSourceMetadata::new(TileFormat::MVT, TileCompression::Gzip, Traversal::ANY, None),
		)?;

		let runtime = TilesRuntime::default();

		let mut data_writer = DataWriterBlob::new()?;
		PMTilesWriter::write_to_writer(&mut mock_reader, &mut data_writer, runtime.clone()).await?;

		let data_reader = DataReaderBlob::from(data_writer);
		let mut reader = PMTilesReader::open_data(Box::new(data_reader), runtime).await?;
		MockWriter::write(&mut reader).await?;

		Ok(())
	}

	#[context("test: PMTiles tile ordering (Hilbert & offsets)")]
	#[tokio::test]
	async fn tiles_written_in_order() -> Result<()> {
		let mut tile_pyramid = TilePyramid::new_empty();
		tile_pyramid.insert_bbox(&TileBBox::from_min_and_max(15, 4090, 4090, 4139, 4139)?)?;
		tile_pyramid.insert_bbox(&TileBBox::from_min_and_max(14, 250, 250, 260, 260)?)?;

		let mut mock_reader = MockReader::new_mock(
			tile_pyramid,
			TileSourceMetadata::new(TileFormat::MVT, TileCompression::Uncompressed, Traversal::ANY, None),
		)?;

		let runtime = TilesRuntime::default();

		let mut data_writer = DataWriterBlob::new()?;
		PMTilesWriter::write_to_writer(&mut mock_reader, &mut data_writer, runtime.clone()).await?;

		let data_reader = DataReaderBlob::from(data_writer);
		let reader = PMTilesReader::open_data(Box::new(data_reader), runtime).await?;

		let entries = reader.tile_entries()?;
		let entries_vec = entries.iter().collect::<Vec<_>>();

		// With dedup + merge_runs, all identical MVT tiles share one blob
		// and consecutive Hilbert-indexed entries are merged into runs.
		// Verify tile IDs are in ascending order.
		let mut prev_tile_id = 0;
		for (i, entry) in entries_vec.iter().enumerate() {
			if i > 0 {
				assert!(
					entry.tile_id > prev_tile_id,
					"Tile IDs are not in order: {} <= {}",
					entry.tile_id,
					prev_tile_id
				);
			}
			prev_tile_id = entry.tile_id + u64::from(entry.run_length.max(1)) - 1;
		}

		// Total addressed tiles should match the original tile count
		assert_eq!(entries.tile_count(), 2621); // 50×50 at z15 + 11×11 at z14
		Ok(())
	}
}
