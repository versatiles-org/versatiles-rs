//! Write tiles and metadata into a `.versatiles` container.
//!
//! The `VersaTilesWriter` produces a valid `.versatiles` file from any [`TileSource`]
//! source. It serializes `TileJSON` metadata, groups tiles into fixed **256×256 blocks**,
//! compresses per-block tile indices and metadata, and writes a compact binary structure
//! ready for fast random access by the [`VersaTilesReader`](crate::container::versatiles::VersaTilesReader).
//!
//! ## File layout
//! ```notest
//! [ FileHeader | meta_blob | block_index_blob | blocks... ]
//! ```
//! Each block contains:
//! - a Brotli-compressed **tile index** (mapping tile IDs to byte ranges)
//! - a contiguous sequence of tile blobs in the reader’s `tile_format` and `tile_compression`
//!
//! ## Behavior
//! - All tiles are grouped in 256×256 blocks (`Traversal::new_any_size(256, 256)`).
//! - The header is written twice: once before, and once after writing metadata and blocks.
//! - Metadata (`TileJSON`) and block indices are compressed using Brotli for storage efficiency.
//! - The writer supports both raster and vector tile formats.
//!
//! ## Example
//! ```rust,no_run
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use std::path::Path;
//! use anyhow::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let runtime = TilesRuntime::default();
//!
//!     // Open an MBTiles source
//!     let path_in = Path::new("../testdata/berlin.mbtiles");
//!     let mut reader = MBTilesReader::open(&path_in, runtime.clone())?;
//!
//!     // Write as a .versatiles container
//!     let path_out = std::env::temp_dir().join("berlin.versatiles");
//!     VersaTilesWriter::write_to_path(&mut reader, &path_out, runtime).await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Errors
//! Returns errors if writing fails, compression fails, or if metadata or bounding box
//! information is invalid.

use super::types::{BlockBuilder, BlockIndex, FileHeader};
use crate::{TileSource, TileSourceTraverseExt, TilesRuntime, TilesWriter, Traversal};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt, channel::mpsc, try_join};
use std::time::Instant;
use versatiles_core::{
	compression::compress,
	io::DataWriterTrait,
	types::{Blob, ByteRange, TileCompression, TileCoord},
};

/// Messages streamed from the parallel block builders to the single serial writer.
///
/// Tiles are sent individually (not as a whole pre-built block) so the writer can
/// append them straight to the output as they arrive. This bounds peak memory to a
/// small number of in-flight tiles instead of materializing an entire block — which
/// for dense low-zoom DEM can be many gigabytes.
enum BlockMessage {
	/// Begin a new block at the given zoom level.
	Start(u8),
	/// A compressed tile belonging to the current block.
	Tile(TileCoord, Blob),
	/// The current block is complete; finalize and write its index.
	End,
}
use versatiles_derive::context;

/// Number of compressed tiles that may sit in the writer queue at once.
///
/// Tiles are compressed in parallel off the writer's critical path, then streamed
/// individually to the single serial writer which appends them straight to the output.
/// This buffer decouples the parallel compressor from the serial writer; peak memory is
/// roughly this many compressed tiles (so ≈ `depth × tile size`), independent of how
/// large a block is. Override with `VERSATILES_WRITE_TILE_BUFFER`.
fn tile_buffer_size() -> usize {
	std::env::var("VERSATILES_WRITE_TILE_BUFFER")
		.ok()
		.and_then(|s| s.trim().parse::<usize>().ok())
		.filter(|&n| n > 0)
		.unwrap_or(64)
}

/// Writer for `.versatiles` containers.
///
/// Serializes a [`TileSource`] source into a compact binary container optimized
/// for fast random tile access. The writer:
/// - compresses metadata and block indices with Brotli
/// - organizes tiles into 256×256 blocks
/// - writes the header twice to ensure integrity
///
/// The resulting file can be read by the [`VersaTilesReader`](crate::container::versatiles::VersaTilesReader).
pub struct VersaTilesWriter {}

#[async_trait]
impl TilesWriter for VersaTilesWriter {
	/// Convert tiles from a [`TileSource`] and write them to a [`DataWriterTrait`].
	///
	/// This method writes the file header, followed by metadata, blocks, and an updated
	/// header containing the final byte ranges. It compresses metadata and block indices
	/// using Brotli and enforces uniform tile format and compression across all tiles.
	///
	/// # Errors
	/// Returns an error if writing, compression, or bounding box validation fails.
	#[context("writing VersaTiles to DataWriter")]
	async fn write_to_writer(
		reader: &mut dyn TileSource,
		writer: &mut dyn DataWriterTrait,
		runtime: TilesRuntime,
	) -> Result<()> {
		// Finalize the configuration
		let parameters = reader.metadata();
		log::trace!("convert_from - reader.parameters: {parameters:?}");

		let tile_compression = *parameters.tile_compression();

		// Get the tile pyramid (computed lazily by the source).
		let tile_pyramid = reader.tile_pyramid().await?;
		log::trace!("convert_from - tile_pyramid: {tile_pyramid:#}");

		// Create the file header, preferring TileJSON values over pyramid-calculated ones
		let tilejson = reader.tilejson();
		let zoom_min = tilejson
			.zoom_min()
			.or(tile_pyramid.level_min())
			.ok_or(anyhow!("invalid minzoom"))?;
		let zoom_max = tilejson
			.zoom_max()
			.or(tile_pyramid.level_max())
			.ok_or(anyhow!("invalid maxzoom"))?;
		let bbox = tilejson
			.bounds
			.or(tile_pyramid.geo_bbox())
			.ok_or(anyhow!("invalid geo bounding box"))?;
		let mut header = FileHeader::new(*parameters.tile_format(), tile_compression, [zoom_min, zoom_max], &bbox)?;

		// Convert the header to a blob and write it
		let blob: Blob = header.to_blob()?;
		log::trace!("write header");
		writer.append(&blob)?;

		log::trace!("write meta");
		header.meta_range = Self::write_meta(reader, writer, tile_compression).await?;

		log::trace!("write blocks");
		header.blocks_range = Self::write_blocks(reader, writer, tile_compression, runtime).await?;

		log::trace!("update header");
		let blob: Blob = header.to_blob()?;
		writer.write_start(&blob)?;

		Ok(())
	}
}

impl VersaTilesWriter {
	/// Write the `TileJSON` metadata as a Brotli-compressed blob to the writer.
	///
	/// Returns the byte range where the metadata was written.
	#[context("Failed to write metadata")]
	async fn write_meta(
		reader: &dyn TileSource,
		writer: &mut dyn DataWriterTrait,
		compression: TileCompression,
	) -> Result<ByteRange> {
		let meta: Blob = reader.tilejson().into();
		let compressed = compress(meta, &compression)?;

		writer.append(&compressed)
	}

	/// Write all tile blocks and their Brotli-compressed indices.
	///
	/// Traverses the reader in 256×256 blocks, writes tiles into each block, and appends
	/// the resulting block index at the end of the file.
	///
	/// Returns the byte range covering the block index blob.
	#[context("Failed to write blocks")]
	async fn write_blocks(
		reader: &mut dyn TileSource,
		writer: &mut dyn DataWriterTrait,
		tile_compression: TileCompression,
		runtime: TilesRuntime,
	) -> Result<ByteRange> {
		if reader.tile_pyramid().await?.is_empty() {
			return Ok(ByteRange::empty());
		}

		// Pipeline: tiles are compressed in parallel off the writer's critical path, then
		// streamed individually to a single serial writer over a bounded channel. The
		// writer appends each tile straight to the output and builds the block index
		// incrementally, so peak memory is a handful of in-flight tiles — NOT a whole
		// block (which for dense low-zoom DEM can be many gigabytes and would OOM).
		let (tx, mut rx) = mpsc::channel::<BlockMessage>(tile_buffer_size());

		// Producer: compress tiles in parallel and stream them, block by block.
		let produce = async move {
			reader
				.traverse_all_tiles(
					&Traversal::new_any_size(256, 256)?,
					move |bbox, stream| {
						let mut tx = tx.clone();

						Box::pin(async move {
							log::trace!("start processing block at {bbox:?}");
							tx.send(BlockMessage::Start(bbox.level()))
								.await
								.map_err(|_| anyhow!("writer stopped accepting blocks"))?;

							// Compress tiles in parallel, then forward each to the serial writer.
							stream
								.map_parallel_try(move |coord, tile| {
									// Time each tile; trace the slow ones — a single large tile
									// re-encoding single-threaded can hold a core for a long time
									// (e.g. max-quality DEM at low zoom).
									let started = Instant::now();
									let blob = tile.into_blob(&tile_compression)?;
									let elapsed = started.elapsed();
									if elapsed.as_secs_f64() > 1.0 {
										log::trace!(
											"writer: slow tile encode {coord:?}: {elapsed:?} -> {} bytes",
											blob.len()
										);
									}
									Ok(blob)
								})
								.unwrap_results()
								.for_each_async_try(|coord, blob| {
									let mut tx = tx.clone();
									// Backpressure here bounds in-flight memory; the writer runs concurrently.
									async move {
										tx.send(BlockMessage::Tile(coord, blob))
											.await
											.map_err(|_| anyhow!("writer stopped accepting tiles"))
									}
								})
								.await?;

							tx.send(BlockMessage::End)
								.await
								.map_err(|_| anyhow!("writer stopped accepting blocks"))?;
							Ok(())
						})
					},
					runtime.clone(),
				)
				.await?;
			// Dropping the last sender closes the channel so the writer can finish.
			Ok::<(), anyhow::Error>(())
		};

		// Consumer: the single serial writer. Builds each block directly into the output
		// via `BlockBuilder` (which appends tiles immediately and keeps only the small
		// per-tile index in memory), so no whole-block buffer is ever materialized.
		// (Network sinks keep their connection alive across idle gaps via `SftpKeepalive`.)
		let consume = async {
			let mut block_index = BlockIndex::new_empty();
			while let Some(message) = rx.next().await {
				let BlockMessage::Start(level) = message else {
					return Err(anyhow!("writer protocol error: expected block start"));
				};
				// `BlockBuilder` borrows the writer for the duration of this block only,
				// writing tiles straight to the output as they arrive.
				let mut block_builder = BlockBuilder::new(level, writer)?;
				let mut tile_count: u64 = 0;
				loop {
					match rx.next().await {
						Some(BlockMessage::Tile(coord, blob)) => {
							block_builder.write_tile(coord, blob)?;
							tile_count += 1;
						}
						Some(BlockMessage::End) => break,
						Some(BlockMessage::Start(_)) => return Err(anyhow!("writer protocol error: nested block start")),
						None => return Err(anyhow!("producer dropped mid-block")),
					}
				}
				if let Some(block) = block_builder.finalize()? {
					log::trace!("writer: wrote block ({tile_count} tiles) to output");
					block_index.insert_block(block);
				}
			}
			log::trace!("writer: input drained, finalizing block index");
			Ok::<BlockIndex, anyhow::Error>(block_index)
		};

		let ((), block_index) = try_join!(produce, consume)?;

		// write the block index
		let range = writer.append(&block_index.to_brotli_blob()?)?;

		Ok(range)
	}
}
