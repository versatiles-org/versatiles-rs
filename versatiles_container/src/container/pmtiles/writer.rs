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
	path::{Path, PathBuf},
	sync::Arc,
};

use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use futures::lock::Mutex;
use versatiles_core::{
	compression::compress,
	io::{DataReaderFile, DataReaderTrait, DataWriterFile, DataWriterTrait},
	types::{Blob, ByteRange, TileCompression},
	utils::HilbertIndex,
};
use versatiles_derive::context;

use super::types::{EntriesV3, EntryV3, HeaderV3, PMTilesCompression};
use crate::{
	TileSource, TileSourceMetadata, TileSourceTraverseExt, TilesRuntime, TilesWriter,
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

/// Writer option: buy clustering back with a second pass over the tile data.
const REORDER: &str = "reorder";

/// Writer option: where the second pass puts its temporary file.
const TEMP_DIR: &str = "temp_dir";

#[async_trait]
impl TilesWriter for PMTilesWriter {
	/// A source that cannot supply Hilbert order is refused by default; these
	/// are the two ways to accept one, and they trade against each other.
	/// `allow_unclustered` costs nothing and gives up clustering;
	/// `reorder` keeps clustering and costs an extra full pass over the tile
	/// data plus temporary disk of the same size. `temp_dir` says where that
	/// temporary goes.
	fn supported_options() -> &'static [&'static str] {
		&[ALLOW_UNCLUSTERED, REORDER, TEMP_DIR]
	}

	/// Writes to `path`, which is also the only place the output's location is
	/// known — a `reorder` pass borrows its directory so the temporary file
	/// lands on the same filesystem as the output rather than on whatever `/tmp`
	/// happens to be.
	async fn write_to_path(reader: &mut dyn TileSource, path: &Path, runtime: TilesRuntime) -> Result<()> {
		let mut writer = DataWriterFile::from_path(path)?;
		Self::write(reader, &mut writer, runtime, path.parent()).await
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
		// A generic sink (e.g. SFTP) has no local directory to borrow.
		Self::write(reader, writer, runtime, None).await
	}
}

impl PMTilesWriter {
	#[context("writing PMTiles")]
	async fn write(
		reader: &mut dyn TileSource,
		writer: &mut dyn DataWriterTrait,
		runtime: TilesRuntime,
		output_dir: Option<&Path>,
	) -> Result<()> {
		const INTERNAL_COMPRESSION: TileCompression = TileCompression::Gzip;

		let parameters = reader.metadata().clone();

		let plan = OrderPlan::negotiate(&parameters, &runtime)?;
		let (traversal, reorder, clustered) = (plan.traversal, plan.reorder, plan.clustered);

		let entries = EntriesV3::new();

		writer.set_position(16384)?;

		let tilejson = reader.tilejson();
		let tile_pyramid = reader.tile_pyramid().await?;
		let mut header = HeaderV3::from_parameters(&parameters, tile_pyramid.as_ref(), tilejson);

		let mut metadata: Blob = tilejson.into();
		metadata = compress(metadata, &INTERNAL_COMPRESSION)?;
		header.metadata = writer.append(&metadata)?;

		let tile_data_start = writer.position()?;

		// With `reorder`, pass 1 writes tile data to a local temporary instead of
		// the real sink, so pass 2 can copy it back in tile-id order. The
		// temporary has to be local and readable — `DataWriterTrait` is
		// append-only with no way to read back — which is also why it cannot be a
		// second remote file when the sink is SFTP.
		let temp = if reorder {
			Some(TempTileData::create(&runtime, output_dir)?)
		} else {
			None
		};
		let mut temp_writer = match &temp {
			Some(temp) => Some(DataWriterFile::from_path(temp.path())?),
			None => None,
		};
		let tile_data_writer: &mut dyn DataWriterTrait = match temp_writer.as_mut() {
			Some(w) => w,
			None => &mut *writer,
		};
		// Ranges are stored relative to the tile-data section, which starts at 0
		// in the temporary and after the metadata in the real sink.
		let pass1_origin = if reorder { 0 } else { tile_data_start };

		let writer_mutex = Arc::new(Mutex::new(tile_data_writer));
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
								let range = writer.append(blob)?.shifted_backward(pass1_origin)?;
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
		let tile_contents_count = dedup_map.lock().await.len() as u64;
		drop(writer_mutex.lock().await);
		drop(temp_writer);

		if let Some(temp) = &temp {
			copy_tiles_in_id_order(&mut entries, temp, writer, tile_data_start, &runtime).await?;
		}

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

/// How the writer will satisfy — or decline — PMTiles' ordering requirement.
struct OrderPlan {
	/// The order to ask the source for.
	traversal: Traversal,
	/// Whether a second pass will put the tile data into tile-id order.
	reorder: bool,
	/// Whether the finished archive may claim `clustered = true`.
	clustered: bool,
}

impl OrderPlan {
	/// A PMTiles archive is only *physically* clustered if tiles arrive in
	/// Hilbert order; the directory does not depend on it, because
	/// `EntriesV3::build_directory` sorts by tile id itself. So arrival order
	/// decides one thing: whether `header.clustered = true` would be truthful.
	///
	/// Decided before any tile is read — on a real pipeline the alternative is
	/// failing minutes in.
	fn negotiate(parameters: &TileSourceMetadata, runtime: &TilesRuntime) -> Result<Self> {
		let required = Traversal::new(TraversalOrder::PMTiles, 1, 64)?;
		let available = parameters.traversal();
		let source_is_ordered = available.can_translate_to(&required);

		let allow_unclustered = runtime.writer_option_bool(ALLOW_UNCLUSTERED)?.unwrap_or(false);
		let reorder_requested = runtime.writer_option_bool(REORDER)?.unwrap_or(false);

		// The two opt-ins ask for different outcomes, so asking for both is a
		// mistake rather than a preference to resolve silently.
		ensure!(
			!(allow_unclustered && reorder_requested),
			"`{ALLOW_UNCLUSTERED}` and `{REORDER}` ask for different things: one gives up clustering \
			 to write in a single pass, the other keeps it by writing the tile data twice. Pick one."
		);

		// Reordering an already-ordered source would cost a full extra pass and
		// change nothing, so it is skipped — but silently skipping it would look
		// like the option was ignored.
		let reorder = reorder_requested && !source_is_ordered;
		if reorder_requested && source_is_ordered {
			log::info!(
				"`{REORDER}` was requested but this source already produces Hilbert order; skipping the extra pass"
			);
		}

		let clustered = source_is_ordered || reorder;

		ensure!(
			clustered || allow_unclustered,
			"cannot write PMTiles: this source produces tiles in {} order, and PMTiles stores them \
			 in Hilbert order. Operations that build lower zoom levels from higher ones, such as \
			 `raster_overview`, force this order, and reordering while writing would mean holding \
			 the whole tileset in memory.\n\
			 \n\
			 Three ways forward:\n\
			 - write .versatiles or .mbtiles instead — they accept any order, and cost nothing\n\
			 - `--writer-option {REORDER}=true` — keeps the archive clustered, at the cost of \
			 writing the tile data twice and temporary disk the size of the output (see \
			 `{TEMP_DIR}`)\n\
			 - `--writer-option {ALLOW_UNCLUSTERED}=true` — a single pass, but the tile data is not \
			 physically clustered, so serving it needs more range requests\n\
			 \n\
			 The archive is valid and reads correctly whichever you pick. None is chosen for you \
			 because which cost is acceptable depends on the job.",
			available.order()
		);

		Ok(Self {
			// A reorder pass reads in whatever order the source gives and sorts
			// afterwards, so only an already-ordered source wants the chunked
			// Hilbert traversal.
			traversal: if source_is_ordered { required } else { Traversal::ANY },
			reorder,
			clustered,
		})
	}
}

/// The temporary file a `reorder` pass writes its tile data to.
///
/// Removed on drop, so it goes on success, on error, and on most panics — the
/// file is the size of the whole tile data section, which is exactly the thing
/// not to leave behind.
struct TempTileData {
	file: tempfile::NamedTempFile,
}

impl TempTileData {
	/// Places the temporary next to the output by default.
	///
	/// The output's directory is the one location known to have room for
	/// something the size of the output, and keeping both on one filesystem
	/// avoids a cross-device copy. `temp_dir=` overrides it; the system
	/// temporary directory is the last resort, which is what a generic sink
	/// (SFTP) gets since it has no local directory to borrow.
	fn create(runtime: &TilesRuntime, output_dir: Option<&Path>) -> Result<Self> {
		let dir: PathBuf = match runtime.writer_option(TEMP_DIR) {
			Some(dir) => PathBuf::from(dir),
			None => match output_dir {
				Some(dir) if dir.as_os_str().is_empty() => PathBuf::from("."),
				Some(dir) => dir.to_path_buf(),
				None => std::env::temp_dir(),
			},
		};

		let file = tempfile::Builder::new()
			.prefix(".versatiles-reorder-")
			.suffix(".tmp")
			.tempfile_in(&dir)
			.with_context(|| format!("could not create the reorder temporary file in {dir:?}"))?;
		log::debug!("reorder: writing tile data to {:?} first", file.path());
		Ok(Self { file })
	}

	fn path(&self) -> &Path {
		self.file.path()
	}
}

/// Copies tile data from the temporary into the real sink in tile-id order, and
/// rewrites every entry to point at its new home.
///
/// This is what makes `clustered = true` truthful for a source that could not
/// supply Hilbert order: the directory never depended on arrival order, since
/// `EntriesV3::build_directory` sorts by tile id itself, so only the physical
/// layout of the tile data had to be fixed.
///
/// Deduplication is preserved rather than expanded. The writer maps content hash
/// to one byte range, so several tile ids can share a range; the first entry to
/// reference a temporary range copies the blob, and every later entry pointing at
/// the same temporary range is remapped to the same destination. Expanding them
/// into copies would silently undo dedup and inflate the archive.
async fn copy_tiles_in_id_order(
	entries: &mut EntriesV3,
	temp: &TempTileData,
	writer: &mut dyn DataWriterTrait,
	tile_data_start: u64,
	runtime: &TilesRuntime,
) -> Result<()> {
	let reader = DataReaderFile::open(temp.path())?;

	// Sorting here rather than relying on `build_directory` to do it later: the
	// copy has to happen in tile-id order, and the directory is built from the
	// same vector afterwards.
	entries.sort_by_tile_id();

	let progress = runtime.create_progress("reordering tiles", entries.len() as u64);
	let mut moved: HashMap<ByteRange, ByteRange> = HashMap::new();

	for entry in entries.iter_mut() {
		let destination = if let Some(&range) = moved.get(&entry.range) {
			range
		} else {
			let blob = reader.read_range(&entry.range).await?;
			let range = writer.append(&blob)?.shifted_backward(tile_data_start)?;
			moved.insert(entry.range, range);
			range
		};
		entry.range = destination;
		progress.inc(1);
	}

	progress.finish();
	log::debug!(
		"reorder: copied {} unique blob(s) for {} entries",
		moved.len(),
		entries.len()
	);
	Ok(())
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
		// Every way forward has to be named here, so the choice is visible where
		// it is made rather than only in the documentation.
		assert!(err.contains(".versatiles"), "should offer another container: {err}");
		assert!(err.contains("reorder=true"), "should offer the reordering pass: {err}");
		assert!(
			err.contains("allow_unclustered=true"),
			"should offer the unclustered opt-in: {err}"
		);
		// Each option is only a real choice if its cost is stated too.
		assert!(err.contains("twice"), "should state reorder's cost: {err}");
		assert!(
			err.contains("range requests"),
			"should state the unclustered cost: {err}"
		);
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

	async fn write_with(reader: &mut MockReader, options: &[(&str, &str)]) -> Result<Blob> {
		let mut builder = TilesRuntime::builder().silent_progress(true);
		for (key, value) in options {
			builder = builder.writer_option(key, value);
		}
		write_to_blob(reader, builder.build()).await
	}

	#[tokio::test]
	async fn reorder_accepts_an_unordered_source_and_keeps_it_clustered() -> Result<()> {
		let blob = write_with(&mut depth_first_source()?, &[("reorder", "true")]).await?;
		let header = read_header(&blob)?;
		assert!(
			header.clustered,
			"a reordered archive is physically in tile-id order, so it may say so"
		);
		assert!(header.addressed_tiles_count > 0);
		Ok(())
	}

	#[tokio::test]
	async fn reorder_preserves_deduplication() -> Result<()> {
		// The hazard #231 names: several tile ids share one byte range, and the
		// copy pass must keep them sharing rather than writing a copy each.
		// MockReader's PNG tiles are a single constant blob, so every tile in the
		// pyramid dedups down to one stored range — dozens of ids sharing it.
		let unclustered = write_with(&mut depth_first_source()?, &[("allow_unclustered", "true")]).await?;
		let reordered = write_with(&mut depth_first_source()?, &[("reorder", "true")]).await?;

		let a = read_header(&unclustered)?;
		let b = read_header(&reordered)?;

		assert_eq!(a.addressed_tiles_count, b.addressed_tiles_count);
		assert_eq!(
			a.tile_contents_count, b.tile_contents_count,
			"reordering must not change how many distinct blobs are stored"
		);
		assert_eq!(a.tile_entries_count, b.tile_entries_count);
		assert_eq!(
			unclustered.len(),
			reordered.len(),
			"expanding dedup would make the reordered archive much larger"
		);
		Ok(())
	}

	#[tokio::test]
	async fn reorder_and_allow_unclustered_together_are_refused() -> Result<()> {
		let err = write_with(
			&mut depth_first_source()?,
			&[("reorder", "true"), ("allow_unclustered", "true")],
		)
		.await
		.unwrap_err();
		let err = format!("{err:#}");
		assert!(err.contains("ask for different things"), "unexpected: {err}");
		assert!(err.contains("Pick one"), "unexpected: {err}");
		Ok(())
	}

	#[tokio::test]
	async fn reorder_is_skipped_when_the_source_is_already_ordered() -> Result<()> {
		// Asking for it costs nothing and changes nothing, rather than paying for
		// a second pass that would reproduce the same layout.
		let mut reader = MockReader::new_mock(
			TilePyramid::new_full_up_to(3),
			TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None),
		)?;
		let with_reorder = write_with(&mut reader, &[("reorder", "true")]).await?;

		let mut reader = MockReader::new_mock(
			TilePyramid::new_full_up_to(3),
			TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None),
		)?;
		let without = write_with(&mut reader, &[]).await?;

		assert!(read_header(&with_reorder)?.clustered);
		assert_eq!(with_reorder.len(), without.len(), "the extra pass should not have run");
		Ok(())
	}

	#[tokio::test]
	async fn reorder_false_still_refuses_an_unordered_source() -> Result<()> {
		assert!(
			write_with(&mut depth_first_source()?, &[("reorder", "false")])
				.await
				.is_err()
		);
		Ok(())
	}

	#[tokio::test]
	async fn an_unusable_temp_dir_names_itself() -> Result<()> {
		let err = write_with(
			&mut depth_first_source()?,
			&[("reorder", "true"), ("temp_dir", "/nonexistent-versatiles-test")],
		)
		.await
		.unwrap_err();
		let err = format!("{err:#}");
		assert!(err.contains("/nonexistent-versatiles-test"), "unexpected: {err}");
		Ok(())
	}

	#[tokio::test]
	async fn reorder_leaves_no_temporary_behind() -> Result<()> {
		let dir = assert_fs::TempDir::new()?;
		let runtime = TilesRuntime::builder()
			.silent_progress(true)
			.writer_option("reorder", "true")
			.writer_option("temp_dir", dir.path().to_str().unwrap())
			.build();
		write_to_blob(&mut depth_first_source()?, runtime).await?;

		let leftovers: Vec<_> = std::fs::read_dir(dir.path())?.collect();
		assert!(
			leftovers.is_empty(),
			"the temporary must be removed after a successful write"
		);
		Ok(())
	}

	#[test]
	fn the_option_is_declared_so_the_registry_accepts_it() {
		for option in ["allow_unclustered", "reorder", "temp_dir"] {
			assert!(
				PMTilesWriter::supported_options().contains(&option),
				"{option} not declared"
			);
		}
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
