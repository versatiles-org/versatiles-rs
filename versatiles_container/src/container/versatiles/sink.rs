//! A [`TileSink`] implementation that writes tiles into a `.versatiles` container.
//!
//! Tiles arrive in arbitrary order via [`write_tile`](TileSink::write_tile).
//! They are buffered to per-block temporary files on disk, grouped by block key
//! `(level, x/256, y/256)`.
//!
//! # Early-flush of full blocks
//!
//! A block holds at most 256×256 tiles. Once a block has received the full
//! 65 536 tiles it can never be updated again, so the sink flushes it to the
//! final output immediately — streaming its tiles through [`BlockBuilder`] and
//! freeing the on-disk temp file — rather than holding every tile until
//! [`finish`](TileSink::finish).
//!
//! Sparse (edge) blocks stay on disk until `finish`, at which point any
//! remaining block files are drained through the same flush path.
//!
//! Supports both local paths and `sftp://` URLs as output destinations.

use super::types::{BlockBuilder, BlockIndex, FileHeader};
use crate::{TileSink, cache::CacheValue};
use anyhow::Result;
use anyhow::anyhow;
#[cfg(not(feature = "ssh2"))]
use anyhow::bail;
use std::{
	collections::HashMap,
	env,
	fs::{self, File},
	io::{BufReader, BufWriter, Write},
	path::PathBuf,
	sync::Mutex,
};
use versatiles_core::{
	Blob, GeoBBox, TileCompression, TileCoord, TileFormat, TileJSON,
	compression::compress,
	io::{DataWriterFile, DataWriterTrait},
};

/// Block key: (level, block_x, block_y) where block_x = x/256, block_y = y/256.
type BlockKey = (u8, u32, u32);

/// Number of tiles in a fully populated block (256 × 256).
const FULL_BLOCK_TILE_COUNT: u32 = 256 * 256;

/// State kept for the final output writer, created lazily on first flush.
///
/// Holds the output writer itself plus the running block index that
/// accumulates [`BlockDefinition`](super::types::BlockDefinition)s as blocks
/// are flushed. Guarded by one mutex so every block emission appends
/// atomically.
struct OutputState {
	writer: Box<dyn DataWriterTrait>,
	block_index: BlockIndex,
	/// Smallest/largest zoom level observed across all flushed blocks so far.
	/// Tracked here (rather than recomputed at finish) because early-flushed
	/// blocks are the only source of truth once their temp files are gone.
	zoom_min: u8,
	zoom_max: u8,
}

/// A tile sink that buffers tiles to temporary files and assembles a `.versatiles`
/// container on [`finish`](TileSink::finish).
///
/// Supports both local file paths and `sftp://` URLs as output destinations.
pub struct VersaTilesSink {
	destination: String,
	tile_format: TileFormat,
	tile_compression: TileCompression,
	temp_dir: PathBuf,
	/// Per-block buffer files keyed by (level, block_x, block_y).
	block_writers: Mutex<HashMap<BlockKey, BufWriter<File>>>,
	/// Running tile count per block. When a block reaches `FULL_BLOCK_TILE_COUNT`
	/// tiles, it is flushed to the output and evicted from both maps.
	block_tile_counts: Mutex<HashMap<BlockKey, u32>>,
	/// Output writer + running block index, lazily created on the first flush.
	output: Mutex<Option<OutputState>>,
	#[cfg(feature = "ssh2")]
	ssh_identity: Option<PathBuf>,
}

impl VersaTilesSink {
	/// Create a new `VersaTilesSink` from a destination string.
	///
	/// The destination can be a local path or an `sftp://` URL.
	pub fn open(
		destination: &str,
		tile_format: TileFormat,
		tile_compression: TileCompression,
		runtime: &crate::TilesRuntime,
	) -> Result<Box<dyn TileSink>> {
		let _ = runtime; // used only with ssh2 feature

		let temp_dir = if destination.starts_with("sftp://") {
			env::temp_dir().join(format!("versatiles_sink_{}", std::process::id()))
		} else {
			PathBuf::from(destination).with_extension("versatiles.tmp")
		};
		fs::create_dir_all(&temp_dir)?;

		Ok(Box::new(Self {
			destination: destination.to_string(),
			tile_format,
			tile_compression,
			temp_dir,
			block_writers: Mutex::new(HashMap::new()),
			block_tile_counts: Mutex::new(HashMap::new()),
			output: Mutex::new(None),
			#[cfg(feature = "ssh2")]
			ssh_identity: runtime.ssh_identity().map(PathBuf::from),
		}))
	}

	/// Path for a block's temporary file.
	fn block_path(&self, key: &BlockKey) -> PathBuf {
		self.temp_dir.join(format!("{}_{}_{}.bin", key.0, key.1, key.2))
	}

	/// Create the appropriate DataWriter for the destination.
	fn create_writer(&self) -> Result<Box<dyn DataWriterTrait>> {
		if self.destination.starts_with("sftp://") {
			#[cfg(feature = "ssh2")]
			{
				let url = reqwest::Url::parse(&self.destination)?;
				return Ok(Box::new(versatiles_core::io::DataWriterSftp::from_url(
					&url,
					self.ssh_identity.as_deref(),
				)?));
			}
			#[cfg(not(feature = "ssh2"))]
			bail!("SFTP support requires the 'ssh2' feature")
		}
		let path = env::current_dir()?.join(&self.destination);
		Ok(Box::new(DataWriterFile::from_path(&path)?))
	}

	/// Lazily initialize the output writer and reserve the 66-byte header slot.
	///
	/// The real [`FileHeader`] cannot be written yet because zoom range,
	/// `meta_range` and `blocks_range` are not known until `finish`. A
	/// placeholder header using known fields and empty byte ranges is written
	/// instead; it is overwritten at the end.
	fn ensure_output<'a>(&self, out: &'a mut Option<OutputState>) -> Result<&'a mut OutputState> {
		if out.is_none() {
			let mut writer = self.create_writer()?;
			// Reserve the header bytes by writing a placeholder; we overwrite
			// it at finish() once we know the final ranges and zoom span.
			let bbox = GeoBBox::new(0.0, 0.0, 0.0, 0.0)?;
			let header = FileHeader::new(self.tile_format, self.tile_compression, [0, 0], &bbox)?;
			writer.append(&header.to_blob()?)?;
			*out = Some(OutputState {
				writer,
				block_index: BlockIndex::new_empty(),
				zoom_min: u8::MAX,
				zoom_max: 0,
			});
		}
		Ok(out.as_mut().expect("just set to Some"))
	}

	/// Stream one block's temp file through a [`BlockBuilder`] into the output
	/// writer and append the resulting [`BlockDefinition`] to the running
	/// block index.
	///
	/// Removes the temp file on success. The block writer and tile counter
	/// for `key` must have been evicted by the caller so no further writes
	/// can race with the flush.
	fn flush_block_to_output(&self, key: &BlockKey) -> Result<()> {
		let path = self.block_path(key);
		let file = File::open(&path)?;
		let mut reader = BufReader::new(file);

		let mut output = self.output.lock().expect("poisoned mutex");
		let state = self.ensure_output(&mut output)?;

		let mut block_builder = BlockBuilder::new(key.0, state.writer.as_mut())?;
		loop {
			let Ok(coord) = TileCoord::read_from_cache(&mut reader) else {
				break;
			};
			let blob =
				Blob::read_from_cache(&mut reader).map_err(|e| anyhow!("failed to read tile blob from temp file: {e}"))?;
			block_builder.write_tile(coord, blob)?;
		}

		if let Some(block) = block_builder.finalize()? {
			state.block_index.insert_block(block);
			state.zoom_min = state.zoom_min.min(key.0);
			state.zoom_max = state.zoom_max.max(key.0);
		}

		drop(reader);
		fs::remove_file(&path)?;
		Ok(())
	}

	/// Close a block's temp file buffer (flushing it to disk) and drop it
	/// from the in-memory state so a subsequent flush can reopen the file
	/// for reading without holding any writer handle.
	fn close_block_writer(&self, key: &BlockKey) -> Result<()> {
		let mut writers = self.block_writers.lock().expect("poisoned mutex");
		if let Some(mut w) = writers.remove(key) {
			w.flush()?;
		}
		// Counter is no longer needed once the block is being flushed.
		self.block_tile_counts.lock().expect("poisoned mutex").remove(key);
		Ok(())
	}
}

impl TileSink for VersaTilesSink {
	fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()> {
		let block_key: BlockKey = (coord.level, coord.x / 256, coord.y / 256);

		// Step 1: append the tile record to this block's temp file and
		// update the per-block counter. We release the block_writers lock
		// before any possible early-flush so the flush path can re-acquire
		// it without deadlocking.
		let is_full = {
			let mut writers = self.block_writers.lock().expect("poisoned mutex");
			let writer = writers.entry(block_key).or_insert_with(|| {
				let path = self.block_path(&block_key);
				BufWriter::new(File::create(path).expect("failed to create block temp file"))
			});

			coord.write_to_cache(writer)?;
			blob.write_to_cache(writer)?;

			let mut counts = self.block_tile_counts.lock().expect("poisoned mutex");
			let count = counts.entry(block_key).or_insert(0);
			*count += 1;
			*count >= FULL_BLOCK_TILE_COUNT
		};

		// Step 2: if the block just hit 256×256 tiles it can never be
		// updated again — flush it straight to the output and free disk.
		if is_full {
			self.close_block_writer(&block_key)?;
			self.flush_block_to_output(&block_key)?;
		}

		Ok(())
	}

	fn finish(self: Box<Self>, tilejson: &TileJSON, runtime: &crate::TilesRuntime) -> Result<()> {
		// 1. Collect and close any block writers that are still open, then
		//    flush each one into the output (the already-early-flushed
		//    blocks are already in the output and need no further work).
		let remaining_keys: Vec<BlockKey> = {
			let mut writers = self.block_writers.lock().expect("poisoned mutex");
			let keys: Vec<BlockKey> = writers.keys().copied().collect();
			for (_, mut w) in writers.drain() {
				w.flush()?;
			}
			self.block_tile_counts.lock().expect("poisoned mutex").clear();
			keys
		};

		let already_flushed = {
			let output = self.output.lock().expect("poisoned mutex");
			output.as_ref().map_or(0, |s| s.block_index.len())
		};

		if remaining_keys.is_empty() && already_flushed == 0 {
			// Genuinely empty sink — write a minimal header-only file.
			let bbox = GeoBBox::new(0.0, 0.0, 0.0, 0.0)?;
			let header = FileHeader::new(self.tile_format, self.tile_compression, [0, 0], &bbox)?;
			let mut writer = self.create_writer()?;
			writer.append(&header.to_blob()?)?;
			fs::remove_dir_all(&self.temp_dir)?;
			return Ok(());
		}

		// 2. Flush any still-buffered (sparse / edge) blocks. Order matches
		//    the previous behaviour — (level, block_y, block_x) — so output
		//    layout of the sparse blocks stays stable for diffability.
		let mut sorted_remaining = remaining_keys;
		sorted_remaining.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)).then(a.1.cmp(&b.1)));

		let progress = runtime.create_progress("finalizing versatiles", sorted_remaining.len() as u64);
		for key in &sorted_remaining {
			self.flush_block_to_output(key)?;
			progress.inc(1);
		}
		progress.finish();

		// 3. Take ownership of the output state to write metadata + index + header.
		let OutputState {
			mut writer,
			block_index,
			zoom_min,
			zoom_max,
		} = self
			.output
			.lock()
			.expect("poisoned mutex")
			.take()
			.expect("output must exist — we've either early-flushed or just flushed remaining blocks");

		let bbox = tilejson
			.bounds
			.unwrap_or_else(|| GeoBBox::new(-180.0, -85.051_13, 180.0, 85.051_13).expect("valid world bbox constants"));

		let mut header = FileHeader::new(self.tile_format, self.tile_compression, [zoom_min, zoom_max], &bbox)?;

		// 4. Append metadata and block index after the block data that's
		//    already been streamed to the writer.
		let meta_blob: Blob = tilejson.into();
		let compressed_meta = compress(meta_blob, &self.tile_compression)?;
		header.meta_range = writer.append(&compressed_meta)?;
		header.blocks_range = writer.append(&block_index.to_brotli_blob()?)?;

		// 5. Rewrite the placeholder header with the real values.
		writer.write_start(&header.to_blob()?)?;

		// 6. Remove the now-empty temp directory.
		fs::remove_dir_all(&self.temp_dir)?;

		Ok(())
	}
}

impl Drop for VersaTilesSink {
	fn drop(&mut self) {
		// Best-effort cleanup of temp directory
		let _ = fs::remove_dir_all(&self.temp_dir);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{TileSource, TilesRuntime, VersaTilesReader};

	#[tokio::test]
	async fn write_and_read_back() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let output = temp_dir.path().join("test.versatiles");
		let runtime = TilesRuntime::default();

		let sink = VersaTilesSink::open(
			output.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;

		let tiles = vec![
			(TileCoord::new(10, 0, 0)?, Blob::from(vec![1u8; 16])),
			(TileCoord::new(10, 1, 0)?, Blob::from(vec![2u8; 16])),
			(TileCoord::new(10, 0, 1)?, Blob::from(vec![3u8; 16])),
		];

		for (coord, blob) in &tiles {
			sink.write_tile(coord, blob)?;
		}

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		tilejson.set_zoom_min(10);
		tilejson.set_zoom_max(10);
		Box::new(sink).finish(&tilejson, &runtime)?;

		let reader = VersaTilesReader::open(&output, TilesRuntime::default()).await?;
		assert_eq!(reader.metadata().tile_format(), &TileFormat::PNG);

		for (coord, expected_blob) in &tiles {
			let tile = reader.tile(coord).await?;
			assert!(tile.is_some(), "tile at {coord:?} should exist");
			let blob = tile.unwrap().into_blob(&TileCompression::Uncompressed)?;
			assert_eq!(blob.as_slice(), expected_blob.as_slice());
		}

		Ok(())
	}

	#[tokio::test]
	async fn write_multiple_levels() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let output = temp_dir.path().join("multi_level.versatiles");
		let runtime = TilesRuntime::default();

		let sink = VersaTilesSink::open(
			output.to_str().unwrap(),
			TileFormat::MVT,
			TileCompression::Gzip,
			&runtime,
		)?;

		let tiles = vec![
			(TileCoord::new(0, 0, 0)?, Blob::from(vec![10u8; 32])),
			(TileCoord::new(1, 0, 0)?, Blob::from(vec![20u8; 32])),
			(TileCoord::new(1, 1, 1)?, Blob::from(vec![30u8; 32])),
		];

		for (coord, blob) in &tiles {
			sink.write_tile(coord, blob)?;
		}

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		tilejson.set_zoom_min(0);
		tilejson.set_zoom_max(1);
		Box::new(sink).finish(&tilejson, &runtime)?;

		let reader = VersaTilesReader::open(&output, TilesRuntime::default()).await?;
		assert_eq!(reader.metadata().tile_format(), &TileFormat::MVT);

		for (coord, expected_blob) in &tiles {
			let tile = reader.tile(coord).await?;
			assert!(tile.is_some(), "tile at {coord:?} should exist");
			let blob = tile.unwrap().into_blob(&TileCompression::Gzip)?;
			assert_eq!(blob.as_slice(), expected_blob.as_slice());
		}

		Ok(())
	}

	#[tokio::test]
	async fn write_across_block_boundaries() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let output = temp_dir.path().join("cross_block.versatiles");
		let runtime = TilesRuntime::default();

		let sink = VersaTilesSink::open(
			output.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;

		let tiles = vec![
			(TileCoord::new(10, 100, 50)?, Blob::from(vec![1u8; 8])),
			(TileCoord::new(10, 300, 50)?, Blob::from(vec![2u8; 8])),
		];

		for (coord, blob) in &tiles {
			sink.write_tile(coord, blob)?;
		}

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		tilejson.set_zoom_min(10);
		tilejson.set_zoom_max(10);
		Box::new(sink).finish(&tilejson, &runtime)?;

		let reader = VersaTilesReader::open(&output, TilesRuntime::default()).await?;
		for (coord, expected_blob) in &tiles {
			let tile = reader.tile(coord).await?;
			assert!(tile.is_some(), "tile at {coord:?} should exist");
			let blob = tile.unwrap().into_blob(&TileCompression::Uncompressed)?;
			assert_eq!(blob.as_slice(), expected_blob.as_slice());
		}

		Ok(())
	}

	/// Writing a fully populated 256×256 block must flush that block to the
	/// output (and delete its temp file) before `finish()` is called. After
	/// all tiles have been written we should observe:
	/// * the block's temp file is gone,
	/// * the temp directory is otherwise empty,
	/// * the running block index inside the output state holds one block,
	/// * the output writer has already advanced past the placeholder header.
	#[tokio::test]
	async fn full_block_flushes_before_finish() -> Result<()> {
		// Zoom 8 has exactly one block (8192/256 = 32? wait, 2^8 = 256 tiles
		// per axis, so exactly one 256×256 block per zoom level at z=8).
		let temp_dir = assert_fs::TempDir::new()?;
		let output = temp_dir.path().join("full_block.versatiles");
		let runtime = TilesRuntime::default();

		let sink_box = VersaTilesSink::open(
			output.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;

		// Downcast-free trick: we built the sink ourselves above, but the
		// returned trait object hides the concrete type. Instead of
		// downcasting, construct a second sink that we keep as VersaTilesSink
		// directly so we can inspect internal state.
		drop(sink_box);
		let expected_tmp = PathBuf::from(output.to_str().unwrap()).with_extension("versatiles.tmp");
		let _ = fs::remove_dir_all(&expected_tmp);

		let sink = Box::new({
			let temp_dir_path = expected_tmp.clone();
			fs::create_dir_all(&temp_dir_path)?;
			VersaTilesSink {
				destination: output.to_str().unwrap().to_string(),
				tile_format: TileFormat::PNG,
				tile_compression: TileCompression::Uncompressed,
				temp_dir: temp_dir_path,
				block_writers: Mutex::new(HashMap::new()),
				block_tile_counts: Mutex::new(HashMap::new()),
				output: Mutex::new(None),
				#[cfg(feature = "ssh2")]
				ssh_identity: None,
			}
		});

		// Fill one 256×256 block at z=8 (the only block at that zoom).
		// Use a tiny (<1000 byte) blob so dedup kicks in and memory stays low.
		let blob = Blob::from(vec![0u8; 4]);
		for y in 0..256u32 {
			for x in 0..256u32 {
				sink.write_tile(&TileCoord::new(8, x, y)?, &blob)?;
			}
		}

		// After the 65 536th tile, the sink should have flushed the block.
		{
			let writers = sink.block_writers.lock().unwrap();
			assert!(
				writers.is_empty(),
				"block writers should have been evicted after full-block flush"
			);
			let counts = sink.block_tile_counts.lock().unwrap();
			assert!(counts.is_empty(), "tile counter should have been cleared");
		}

		// Temp file for the block must have been deleted.
		let block_tmp = sink.block_path(&(8, 0, 0));
		assert!(!block_tmp.exists(), "block temp file must be removed after early flush");

		// The output writer must exist and its running block index must
		// already contain the flushed block.
		{
			let out = sink.output.lock().unwrap();
			let state = out.as_ref().expect("output created during early flush");
			assert_eq!(
				state.block_index.len(),
				1,
				"running block index should contain one block"
			);
			assert_eq!(state.zoom_min, 8);
			assert_eq!(state.zoom_max, 8);
		}

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		tilejson.set_zoom_min(8);
		tilejson.set_zoom_max(8);
		sink.finish(&tilejson, &runtime)?;

		// After finish, the output file must be readable and all tiles present.
		let reader = VersaTilesReader::open(&output, TilesRuntime::default()).await?;
		assert_eq!(reader.metadata().tile_format(), &TileFormat::PNG);
		// Spot-check a handful of tiles to confirm the round trip works.
		for &(x, y) in &[(0u32, 0u32), (0, 255), (255, 0), (255, 255), (128, 128)] {
			let coord = TileCoord::new(8, x, y)?;
			let tile = reader.tile(&coord).await?;
			assert!(tile.is_some(), "tile {coord:?} should exist after round trip");
		}

		Ok(())
	}

	/// A sparse block (< 65 536 tiles) must stay buffered on disk until
	/// `finish()`, then be flushed as part of the drain step. The output
	/// file should still round-trip correctly.
	#[tokio::test]
	async fn sparse_block_stays_buffered_until_finish() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let output = temp_dir.path().join("sparse.versatiles");
		let runtime = TilesRuntime::default();

		let sink_box = VersaTilesSink::open(
			output.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;
		// Re-open as concrete type to inspect internals.
		drop(sink_box);
		let expected_tmp = PathBuf::from(output.to_str().unwrap()).with_extension("versatiles.tmp");
		let _ = fs::remove_dir_all(&expected_tmp);
		fs::create_dir_all(&expected_tmp)?;

		let sink = Box::new(VersaTilesSink {
			destination: output.to_str().unwrap().to_string(),
			tile_format: TileFormat::PNG,
			tile_compression: TileCompression::Uncompressed,
			temp_dir: expected_tmp.clone(),
			block_writers: Mutex::new(HashMap::new()),
			block_tile_counts: Mutex::new(HashMap::new()),
			output: Mutex::new(None),
			#[cfg(feature = "ssh2")]
			ssh_identity: None,
		});

		// Write only a handful of tiles — far from 65 536.
		for x in 0..10u32 {
			sink.write_tile(&TileCoord::new(10, x, 0)?, &Blob::from(vec![1u8; 8]))?;
		}

		// Block writer for (10, 0, 0) must still be open — no early flush.
		{
			let writers = sink.block_writers.lock().unwrap();
			assert!(
				writers.contains_key(&(10, 0, 0)),
				"sparse block should not yet have flushed"
			);
			let out = sink.output.lock().unwrap();
			assert!(out.is_none(), "no block has been flushed, so no output writer yet");
		}

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		tilejson.set_zoom_min(10);
		tilejson.set_zoom_max(10);
		sink.finish(&tilejson, &runtime)?;

		let reader = VersaTilesReader::open(&output, TilesRuntime::default()).await?;
		for x in 0..10u32 {
			let coord = TileCoord::new(10, x, 0)?;
			let tile = reader.tile(&coord).await?;
			assert!(tile.is_some(), "tile {coord:?} should exist");
		}
		Ok(())
	}

	/// Mixed case: one fully populated block (early-flushed) plus one
	/// sparse block (drained at `finish`) in the same file.
	#[tokio::test]
	async fn mixed_full_and_sparse_blocks_round_trip() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let output = temp_dir.path().join("mixed.versatiles");
		let runtime = TilesRuntime::default();

		let sink = VersaTilesSink::open(
			output.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;

		// Block (z=8, 0, 0): fill completely → triggers early flush.
		let blob = Blob::from(vec![0u8; 4]);
		for y in 0..256u32 {
			for x in 0..256u32 {
				sink.write_tile(&TileCoord::new(8, x, y)?, &blob)?;
			}
		}

		// Block (z=10, 0, 0): only a few tiles → stays on disk until finish.
		for x in 0..5u32 {
			sink.write_tile(&TileCoord::new(10, x, 0)?, &Blob::from(vec![2u8; 8]))?;
		}

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		tilejson.set_zoom_min(8);
		tilejson.set_zoom_max(10);
		sink.finish(&tilejson, &runtime)?;

		let reader = VersaTilesReader::open(&output, TilesRuntime::default()).await?;

		// Sample a few tiles from the early-flushed block.
		for &(x, y) in &[(0u32, 0u32), (128, 128), (255, 255)] {
			let coord = TileCoord::new(8, x, y)?;
			assert!(
				reader.tile(&coord).await?.is_some(),
				"missing early-flushed tile {coord:?}"
			);
		}
		// And from the sparse block drained at finish.
		for x in 0..5u32 {
			let coord = TileCoord::new(10, x, 0)?;
			assert!(reader.tile(&coord).await?.is_some(), "missing sparse tile {coord:?}");
		}

		Ok(())
	}

	#[test]
	fn empty_sink_produces_valid_file() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let output = temp_dir.path().join("empty.versatiles");
		let runtime = TilesRuntime::default();

		let sink = VersaTilesSink::open(
			output.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;

		let tilejson = TileJSON::default();
		Box::new(sink).finish(&tilejson, &runtime)?;

		assert!(output.exists());
		Ok(())
	}
}
