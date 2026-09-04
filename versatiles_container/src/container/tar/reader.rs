//! Read tiles and metadata from a `.tar` archive.
//!
//! The `TarTilesReader` scans a tarball for tiles arranged in a `{z}/{x}/{y}.<format>[.<compression>]`
//! layout and optional `TileJSON` metadata files (`meta.json`, `tiles.json`, `metadata.json`)
//! including their compressed variants (`.gz`, `.br`). Non-regular entries are ignored.
//!
//! ## Detected properties
//! - **Tile format** is inferred from the innermost filename extension (e.g., `.png`, `.webp`, `.pbf`, `.mvt`, `.bin`).
//! - **Transport compression** is inferred from an outer extension (e.g., `.br`, `.gz`), or `Uncompressed` if none.
//! - A **tile pyramid** is computed from all discovered `{z,x,y}` coordinates.
//!
//! All tiles must share the same **format** and **compression**; mixing them returns an error.
//!
//! ## Usage
//! ```rust,no_run
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use std::path::Path;
//! # async fn demo() -> anyhow::Result<()> {
//! let path = Path::new("/absolute/path/to/tiles.tar");
//! let mut reader = TarTilesReader::open(path).await?;
//!
//! // Read one tile
//! if let Some(mut tile) = reader.tile(&TileCoord::new(3, 6, 2)?).await? {
//!     let _blob = tile.as_blob(reader.metadata().tile_compression())?;
//! }
//! # Ok(()) }
//! ```
//!
//! ## Errors
//! Returns errors when the tar cannot be opened or read, when no tiles are found,
//! or when mixed formats/compressions are detected.

use std::{collections::HashMap, fmt::Debug, path::Path, sync::Arc};

use anyhow::{Result, anyhow, ensure};
use async_trait::async_trait;
use futures::StreamExt;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_tar::{Archive, Entry, EntryType};
#[cfg(feature = "cli")]
use versatiles_core::utils::PrettyPrint;
use versatiles_core::{
	Blob, ByteRange, TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream,
	compression::decompress,
	io::{DataReaderFile, DataReaderTrait},
};
use versatiles_derive::context;

use crate::{
	ProgressHandle, SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata, TilesReader, TilesRuntime,
	Traversal,
};

/// Reader for tiles stored inside a tar archive.
///
/// Merges `TileJSON` from recognized metadata files, builds a map from `{z,x,y}` to
/// byte ranges within the archive, infers uniform format/compression, and exposes
/// tiles via [`TileSource`].
pub struct TarTilesReader {
	tilejson: TileJSON,
	name: String,
	reader: Arc<DataReaderFile>,
	tile_map: Arc<HashMap<TileCoord, ByteRange>>,
	metadata: TileSourceMetadata,
}

/// Result of parsing a tile path.
struct ParsedTile {
	coord: TileCoord,
	format: TileFormat,
	compression: TileCompression,
	offset: u64,
	length: u64,
}

/// Try to parse a tile path like `{z}/{x}/{y}.<format>[.<compression>]`.
fn try_parse_tile_path<R: AsyncRead + Unpin>(entry: &Entry<R>, path_vec: &[&str]) -> Result<Option<ParsedTile>> {
	if path_vec.len() != 3 {
		return Ok(None);
	}

	let level = path_vec[0].parse::<u8>()?;
	let x = path_vec[1].parse::<u32>()?;

	let mut filename = String::from(path_vec[2]);
	let compression = TileCompression::from_filename(&mut filename);
	let Some(format) = TileFormat::from_filename(&mut filename) else {
		return Ok(None);
	};

	let y = filename.parse::<u32>()?;
	let coord = TileCoord::new(level, x, y)?;

	Ok(Some(ParsedTile {
		coord,
		format,
		compression,
		offset: entry.raw_file_position(),
		length: entry.effective_size(),
	}))
}

impl TarTilesReader {
	/// Open a tar archive and build an index of tiles and metadata.
	///
	/// Scans regular entries in the archive, recognizing:
	/// - tiles at `{z}/{x}/{y}.<format>[.<compression>]`
	/// - metadata files: `meta.json`, `tiles.json`, `metadata.json` (optionally `.gz`/`.br`)
	///
	/// Determines a uniform tile **format** and **compression**, and computes a tile pyramid
	/// from discovered coordinates.
	///
	/// # Errors
	/// Returns an error if the file cannot be opened, if **no tiles** are found, or if mixed
	/// formats/compressions are encountered.
	#[context("opening tar from path '{}'", path.display())]
	pub async fn open(path: &Path) -> Result<TarTilesReader> {
		Self::open_with_progress(path, None).await
	}

	/// Open a tar archive and build an index of tiles and metadata, with optional progress reporting.
	///
	/// When a `ProgressHandle` is provided, progress is reported in bytes scanned.
	#[context("opening tar from path '{}'", path.display())]
	#[expect(
		clippy::too_many_lines,
		reason = "one linear scan over the tar entries; splitting it would separate parsing from the fields it fills"
	)]
	async fn open_with_progress(path: &Path, progress: Option<&ProgressHandle>) -> Result<TarTilesReader> {
		let reader = DataReaderFile::open(path)?;

		// Set progress total to file size if available
		if let Some(progress) = &progress
			&& let Ok(file_meta) = std::fs::metadata(path)
		{
			progress.set_max_value(file_meta.len());
		}

		// A second handle on the same file: the scan below streams the archive
		// front to back, while `reader` stays for the random-access range reads
		// that serve tiles afterwards.
		let scan_file = tokio::fs::File::open(path)
			.await
			.with_context(|| format!("opening {} for scanning", path.display()))?;
		let mut archive = Archive::new(tokio::io::BufReader::new(scan_file));

		let mut tilejson = TileJSON::default();
		let mut tile_map = HashMap::new();
		let mut tile_format: Option<TileFormat> = None;
		let mut tile_compression: Option<TileCompression> = None;
		let mut entries = archive.entries()?;
		while let Some(entry) = entries.next().await {
			let mut entry = entry?;
			let header = entry.header();
			if header.entry_type() != EntryType::Regular {
				continue;
			}

			// Update progress with current position in the archive
			if let Some(progress) = &progress {
				progress.set_position(entry.raw_file_position());
			}

			let path = entry.path()?.clone();

			// An entry name is arbitrary bytes, and a name that is not UTF-8 makes
			// a perfectly legal archive — so it is a reason to skip an entry, not
			// to end the process. Nothing this reader looks for can be spelled in
			// non-UTF-8: tile paths are `z/x/y.ext` and the metadata names are
			// fixed ASCII.
			let Some(mut path_tmp) = path.iter().map(std::ffi::OsStr::to_str).collect::<Option<Vec<&str>>>() else {
				log::debug!("skipping tar entry with a non-UTF-8 name: {:?}", path.as_os_str());
				continue;
			};

			// An entry named "" has no components at all.
			if path_tmp.is_empty() {
				log::debug!("skipping tar entry with an empty name");
				continue;
			}

			if path_tmp[0] == "." {
				path_tmp.remove(0);
			}

			let path_tmp_string = path_tmp.join("/");
			drop(path);
			let path_vec: Vec<&str> = path_tmp_string.split('/').collect();

			if let Some(tile) = try_parse_tile_path(&entry, &path_vec)? {
				if let Some(f) = &tile_format {
					ensure!(
						f == &tile.format,
						"mixed tile formats in tar, found both {f:?} and {:?}",
						tile.format
					);
				} else {
					tile_format = Some(tile.format);
				}

				if let Some(c) = &tile_compression {
					ensure!(
						c == &tile.compression,
						"mixed tile compressions in tar, found both {c:?} and {:?}",
						tile.compression
					);
				} else {
					tile_compression = Some(tile.compression);
				}

				tile_map.insert(
					tile.coord,
					ByteRange {
						offset: tile.offset,
						length: tile.length,
					},
				);
				continue;
			}

			// One lookup and one read, rather than three arms each calling a
			// closure that borrows `entry`: such a closure cannot be held across
			// the `await` the read now needs.
			let metadata_compression = if path_vec.len() == 1 {
				match path_vec[0] {
					"meta.json" | "tiles.json" | "metadata.json" => Some(TileCompression::Uncompressed),
					"meta.json.gz" | "tiles.json.gz" | "metadata.json.gz" => Some(TileCompression::Gzip),
					"meta.json.br" | "tiles.json.br" | "metadata.json.br" => Some(TileCompression::Brotli),
					_ => None,
				}
			} else {
				None
			};

			if let Some(compression) = metadata_compression {
				// Fallible: a truncated archive fails here, and that is an error
				// about the file rather than a broken assumption about it.
				let mut blob: Vec<u8> = Vec::new();
				entry
					.read_to_end(&mut blob)
					.await
					.with_context(|| format!("reading tar entry {path_tmp_string:?}"))?;
				let blob = decompress(Blob::from(blob), &compression)?;
				tilejson.merge(&TileJSON::try_from_blob_or_default(&blob))?;
				continue;
			}

			log::warn!("unknown file in tar: {path_tmp_string:?}");
		}

		if let Some(progress) = &progress {
			progress.finish();
		}

		if tile_map.is_empty() {
			return Err(anyhow!("no tiles found in tar"));
		}

		let metadata = TileSourceMetadata::new(
			tile_format.ok_or(anyhow!("unknown tile format, can't detect format"))?,
			tile_compression.ok_or(anyhow!("unknown tile compression, can't detect compression"))?,
			Traversal::ANY,
			None,
		);

		Ok(TarTilesReader {
			tilejson,
			// The archive's own path, from the caller. Lossy rather than fatal: this
			// string is only ever shown to a human.
			name: path.to_string_lossy().into_owned(),
			metadata,
			reader: Arc::new(*reader),
			tile_map: Arc::new(tile_map),
		})
	}

	/// Internal helper to look up and read a tile by coordinate.
	async fn lookup_tile(
		coord: &TileCoord,
		tile_map: &HashMap<TileCoord, ByteRange>,
		reader: &DataReaderFile,
		tile_compression: TileCompression,
		tile_format: TileFormat,
	) -> Result<Option<Tile>> {
		if let Some(range) = tile_map.get(coord) {
			let blob = reader.read_range(range).await?;
			Ok(Some(Tile::from_blob(blob, tile_compression, tile_format)))
		} else {
			Ok(None)
		}
	}
}

#[async_trait]
impl TilesReader for TarTilesReader {
	fn supports_data_reader() -> bool {
		false
	}

	async fn open_path(path: &Path, runtime: TilesRuntime) -> Result<SharedTileSource> {
		let progress = runtime.create_progress("scanning tar", 0);
		Ok(Self::open_with_progress(path, Some(&progress)).await?.into_shared())
	}
}

#[async_trait]
impl TileSource for TarTilesReader {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("tar", &self.name)
	}

	/// Returns the parameters of the tiles reader.
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	/// Return the parsed `TileJSON` metadata for this archive.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self
			.metadata
			.get_or_compute_tile_pyramid(|| Ok(TilePyramid::from_tile_coords(self.tile_map.keys().copied())))
	}

	/// Fetch a single tile by XYZ coordinate.
	///
	/// Looks up the coordinate in the prebuilt index and reads the corresponding byte range
	/// from the underlying `DataReaderFile`. Returns `Ok(None)` if the tile is absent.
	///
	/// # Errors
	/// Propagates I/O errors while reading the tar entry.
	#[context("getting tile {:?}", coord)]
	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		log::trace!("tile {coord:?}");
		Self::lookup_tile(
			coord,
			&self.tile_map,
			&self.reader,
			*self.metadata.tile_compression(),
			*self.metadata.tile_format(),
		)
		.await
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("tar::tile_stream {bbox:?}");
		let bbox = self.metadata.intersection_bbox(&bbox);
		let reader = Arc::clone(&self.reader);
		let tile_map = Arc::clone(&self.tile_map);
		let tile_compression = *self.metadata.tile_compression();
		let tile_format = *self.metadata.tile_format();

		Ok(TileStream::from_bbox_async_parallel(bbox, move |coord| {
			let reader = Arc::clone(&reader);
			let tile_map = Arc::clone(&tile_map);
			async move {
				let tile = TarTilesReader::lookup_tile(&coord, &tile_map, &reader, tile_compression, tile_format)
					.await
					.ok()??;
				Some((coord, tile))
			}
		}))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		let tile_map = Arc::clone(&self.tile_map);
		Ok(TileStream::from_bbox_parallel(bbox, move |coord| {
			tile_map.get(&coord).map(|_| ())
		}))
	}

	async fn tile_size_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, u32>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		let tile_map = Arc::clone(&self.tile_map);
		Ok(TileStream::from_bbox_parallel(bbox, move |coord| {
			let range = tile_map.get(&coord)?;
			u32::try_from(range.length).ok()
		}))
	}

	#[cfg(feature = "cli")]
	async fn probe_container(&self, print: &mut PrettyPrint, _runtime: &TilesRuntime) -> Result<()> {
		print.add_key_value("tile count", &self.tile_map.len()).await;
		let total_size: u64 = self.tile_map.values().map(|r| r.length).sum();
		print.add_key_value("total stored size", &total_size).await;
		Ok(())
	}
}

impl Debug for TarTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TarTilesReader")
			.field("parameters", &self.metadata())
			.finish()
	}
}

#[cfg(test)]
pub mod tests {
	use versatiles_core::assert_wildcard;
	#[cfg(feature = "cli")]
	use versatiles_core::utils::PrettyPrint;

	use super::*;
	use crate::{MOCK_BYTES_PBF, MockWriter, make_test_file};

	/// A tar entry name is arbitrary bytes. One that is not UTF-8, or empty,
	/// makes a legal archive — and used to panic the process at
	/// `s.to_str().expect("tar path component is utf-8")`, reachable from
	/// `versatiles probe`, `convert` and a served `.tar` alike.
	#[cfg(unix)]
	#[tokio::test]
	async fn hostile_entry_names_are_skipped_not_fatal() -> Result<()> {
		use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

		use versatiles_core::compression::compress_gzip;

		let dir = assert_fs::TempDir::new()?;
		let path = dir.path().join("hostile.tar");

		let tile = compress_gzip(&Blob::from(MOCK_BYTES_PBF.to_vec()))?;
		let mut builder = tokio_tar::Builder::new_non_terminated(tokio::fs::File::create(&path).await?);

		for name in [OsStr::new("5/3/4.pbf.gz"), OsStr::from_bytes(b"5/3/\xff\xfe.pbf.gz")] {
			let mut header = tokio_tar::Header::new_gnu();
			header.set_size(tile.len());
			header.set_mode(0o644);
			header.set_cksum();
			builder
				.append_data(&mut header, Path::new(name), tile.as_slice())
				.await?;
		}
		builder.finish().await?;
		drop(builder);

		// The archive opens, and the readable tile is still there.
		let reader = TarTilesReader::open(&path).await?;
		let coord = TileCoord::new(5, 3, 4)?;
		assert!(reader.tile_map.contains_key(&coord), "the valid tile was lost");
		assert_eq!(reader.tile_map.len(), 1, "a skipped entry was counted");

		Ok(())
	}

	#[tokio::test]
	async fn reader() -> Result<()> {
		let temp_file = make_test_file(TileFormat::MVT, TileCompression::Gzip, 3, "tar").await?;

		// get tar reader
		let reader = TarTilesReader::open(&temp_file).await?;

		assert_eq!(
			format!("{reader:?}"),
			"TarTilesReader { parameters: TileSourceMetadata { tile_compression: Gzip, tile_format: MVT, traversal: Traversal(AnyOrder,full), preferred_traversal: None, tile_pyramid: RwLock { data: None, poisoned: false, .. } } }"
		);
		assert_wildcard!(reader.source_type().to_string(), "container 'tar' ('*.tar')");
		assert_eq!(
			reader.tilejson().stringify(),
			"{\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
		);
		assert_eq!(
			format!("{:?}", reader.metadata()),
			"TileSourceMetadata { tile_compression: Gzip, tile_format: MVT, traversal: Traversal(AnyOrder,full), preferred_traversal: None, tile_pyramid: RwLock { data: None, poisoned: false, .. } }"
		);
		assert_eq!(reader.metadata().tile_compression(), &TileCompression::Gzip);
		assert_eq!(reader.metadata().tile_format(), &TileFormat::MVT);

		let blob = reader
			.tile(&TileCoord::new(3, 6, 2)?)
			.await?
			.unwrap()
			.into_blob(&TileCompression::Uncompressed)?;
		assert_eq!(blob.as_slice(), MOCK_BYTES_PBF);

		Ok(())
	}

	#[tokio::test]
	async fn all_compressions() -> Result<()> {
		async fn test_compression(compression: TileCompression) -> Result<()> {
			let temp_file = make_test_file(TileFormat::MVT, compression, 2, "tar").await?;

			// get tar reader
			let reader = TarTilesReader::open(&temp_file).await?;

			MockWriter::write(&reader).await?;
			Ok(())
		}

		test_compression(TileCompression::Uncompressed).await?;
		test_compression(TileCompression::Gzip).await?;
		test_compression(TileCompression::Brotli).await?;
		Ok(())
	}

	// Test tile fetching
	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn probe() -> Result<()> {
		let temp_file = make_test_file(TileFormat::MVT, TileCompression::Gzip, 4, "tar").await?;

		let reader = TarTilesReader::open(&temp_file).await?;
		let runtime = TilesRuntime::default();

		let mut printer = PrettyPrint::new();
		reader
			.probe_container(&mut printer.category("container").await, &runtime)
			.await?;
		assert_eq!(
			printer.stringify().await.split('\n').collect::<Vec<_>>(),
			["container:", "  tile count: 341", "  total stored size: 26_257", ""]
		);

		Ok(())
	}

	#[tokio::test]
	async fn empty_tar_file() -> Result<()> {
		let filename = assert_fs::NamedTempFile::new("empty_tar_file.tar")?;
		let file = tokio::fs::File::create(&filename).await?;
		let mut a = tokio_tar::Builder::new_non_terminated(file);
		a.finish().await?;

		assert_eq!(
			TarTilesReader::open(&filename)
				.await
				.unwrap_err()
				.chain()
				.last()
				.unwrap()
				.to_string(),
			"no tiles found in tar"
		);
		Ok(())
	}

	#[tokio::test]
	async fn correct_zxy_scheme() -> Result<()> {
		let filename = assert_fs::NamedTempFile::new("correct_zxy_scheme.tar")?;
		let file = tokio::fs::File::create(&filename).await?;
		let mut a = tokio_tar::Builder::new_non_terminated(file);
		let mut header = tokio_tar::Header::new_gnu();
		header.set_size(6);
		header.set_cksum();
		a.append_data(&mut header, "3/1/2.bin", [3, 1, 4, 1, 5, 9].as_ref())
			.await?;
		a.finish().await?;

		let reader = TarTilesReader::open(&filename).await?;
		assert_eq!(reader.metadata().tile_format(), &TileFormat::BIN);
		assert_eq!(reader.metadata().tile_compression(), &TileCompression::Uncompressed);
		assert_eq!(reader.tile_pyramid().await?.count_tiles(), 1);
		assert_eq!(
			reader
				.tile(&TileCoord::new(3, 1, 2)?)
				.await?
				.unwrap()
				.as_blob(&TileCompression::Uncompressed)?
				.as_slice(),
			[3, 1, 4, 1, 5, 9].as_ref()
		);
		Ok(())
	}

	#[tokio::test]
	async fn tile_stream_matches_individual_reads() -> Result<()> {
		let temp_file = make_test_file(TileFormat::MVT, TileCompression::Gzip, 2, "tar").await?;
		let reader = TarTilesReader::open(&temp_file).await?;

		// Use level 2 bbox (4x4 = 16 tiles)
		let bbox = TileBBox::new_full(2)?;

		// Get all tiles via stream
		let stream = reader.tile_stream(bbox).await?;
		let stream_tiles: Vec<_> = stream.to_vec().await;
		assert_eq!(stream_tiles.len(), 16);

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
