//! A [`TileSink`] implementation that writes tiles into a `.versatiles` container.
//!
//! Tiles arrive in arbitrary order via [`write_tile`](TileSink::write_tile).
//! They are buffered to per-block temporary files on disk, grouped by block key
//! `(level, x/256, y/256)`. On [`finish`](TileSink::finish), blocks are assembled
//! in sorted order using [`BlockBuilder`] and written to the final `.versatiles` file.
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
}

impl TileSink for VersaTilesSink {
	fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()> {
		let block_key: BlockKey = (coord.level, coord.x / 256, coord.y / 256);

		let mut writers = self.block_writers.lock().unwrap();
		let writer = writers.entry(block_key).or_insert_with(|| {
			let path = self.block_path(&block_key);
			BufWriter::new(File::create(path).expect("failed to create block temp file"))
		});

		coord.write_to_cache(writer)?;
		blob.write_to_cache(writer)?;

		Ok(())
	}

	fn finish(self: Box<Self>, tilejson: &TileJSON, runtime: &crate::TilesRuntime) -> Result<()> {
		// 1. Flush and close all block writers, collect block keys
		let block_keys: Vec<BlockKey> = {
			let mut writers = self.block_writers.lock().unwrap();
			let keys: Vec<BlockKey> = writers.keys().copied().collect();
			for (_, mut w) in writers.drain() {
				w.flush()?;
			}
			keys
		};

		if block_keys.is_empty() {
			// No tiles — write an empty file with just the header
			let bbox = GeoBBox::new(0.0, 0.0, 0.0, 0.0)?;
			let header = FileHeader::new(self.tile_format, self.tile_compression, [0, 0], &bbox)?;
			let mut writer = self.create_writer()?;
			writer.append(&header.to_blob()?)?;
			fs::remove_dir_all(&self.temp_dir)?;
			return Ok(());
		}

		// 2. Sort block keys by (level, block_y, block_x)
		let mut sorted_keys = block_keys;
		sorted_keys.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)).then(a.1.cmp(&b.1)));

		// 3. Compute zoom range from block keys
		let zoom_min = sorted_keys.iter().map(|k| k.0).min().unwrap();
		let zoom_max = sorted_keys.iter().map(|k| k.0).max().unwrap();

		// 4. Compute bbox from tilejson or default to world
		let bbox = tilejson
			.bounds
			.unwrap_or_else(|| GeoBBox::new(-180.0, -85.051_13, 180.0, 85.051_13).unwrap());

		// 5. Create output writer and write initial header
		let mut writer = self.create_writer()?;
		let mut header = FileHeader::new(self.tile_format, self.tile_compression, [zoom_min, zoom_max], &bbox)?;
		writer.append(&header.to_blob()?)?;

		// 6. Write metadata
		let meta_blob: Blob = tilejson.into();
		let compressed_meta = compress(meta_blob, self.tile_compression)?;
		header.meta_range = writer.append(&compressed_meta)?;

		// 7. Write blocks with progress reporting
		let mut block_index = BlockIndex::new_empty();
		let progress = runtime.create_progress("finalizing versatiles", sorted_keys.len() as u64);

		for key in &sorted_keys {
			let path = self.block_path(key);
			let file = File::open(&path)?;
			let mut reader = BufReader::new(file);

			let mut block_builder = BlockBuilder::new(key.0, writer.as_mut())?;

			// Read all (TileCoord, Blob) pairs from the temp file
			loop {
				let Ok(coord) = TileCoord::read_from_cache(&mut reader) else {
					break;
				};
				let blob = Blob::read_from_cache(&mut reader)
					.map_err(|e| anyhow!("failed to read tile blob from temp file: {e}"))?;
				block_builder.write_tile(coord, blob)?;
			}

			if let Some(block) = block_builder.finalize()? {
				block_index.insert_block(block);
			}

			// Delete temp file immediately to free disk space
			drop(reader);
			fs::remove_file(&path)?;

			progress.inc(1);
		}

		progress.finish();

		// 8. Write block index
		header.blocks_range = writer.append(&block_index.to_brotli_blob()?)?;

		// 9. Rewrite header with final ranges
		writer.write_start(&header.to_blob()?)?;

		// 10. Remove the now-empty temp directory
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
		tilejson.set_min_zoom(10);
		tilejson.set_max_zoom(10);
		Box::new(sink).finish(&tilejson, &runtime)?;

		let reader = VersaTilesReader::open(&output, TilesRuntime::default()).await?;
		assert_eq!(reader.metadata().tile_format, TileFormat::PNG);

		for (coord, expected_blob) in &tiles {
			let tile = reader.get_tile(coord).await?;
			assert!(tile.is_some(), "tile at {coord:?} should exist");
			let blob = tile.unwrap().into_blob(TileCompression::Uncompressed)?;
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
		tilejson.set_min_zoom(0);
		tilejson.set_max_zoom(1);
		Box::new(sink).finish(&tilejson, &runtime)?;

		let reader = VersaTilesReader::open(&output, TilesRuntime::default()).await?;
		assert_eq!(reader.metadata().tile_format, TileFormat::MVT);

		for (coord, expected_blob) in &tiles {
			let tile = reader.get_tile(coord).await?;
			assert!(tile.is_some(), "tile at {coord:?} should exist");
			let blob = tile.unwrap().into_blob(TileCompression::Gzip)?;
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
		tilejson.set_min_zoom(10);
		tilejson.set_max_zoom(10);
		Box::new(sink).finish(&tilejson, &runtime)?;

		let reader = VersaTilesReader::open(&output, TilesRuntime::default()).await?;
		for (coord, expected_blob) in &tiles {
			let tile = reader.get_tile(coord).await?;
			assert!(tile.is_some(), "tile at {coord:?} should exist");
			let blob = tile.unwrap().into_blob(TileCompression::Uncompressed)?;
			assert_eq!(blob.as_slice(), expected_blob.as_slice());
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
