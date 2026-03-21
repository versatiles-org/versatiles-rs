use super::reader_cache::{ReaderCache, SourceInfo};
use anyhow::{Context, Result, anyhow, ensure};
use async_trait::async_trait;
use futures::{StreamExt, stream};
use std::sync::Arc;
use tokio::sync::Mutex;
use versatiles_container::{
	SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata, TilesConverterParameters, TilesRuntime,
	Traversal, convert_tiles_container_to_str,
};
use versatiles_core::{TileBBox, TileBBoxMap, TileBBoxPyramid, TileCoord, TileFormat, TileJSON, TileStream};
use versatiles_image::traits::{DynamicImageTraitInfo, DynamicImageTraitOperation};

/// Maximum number of container readers kept open simultaneously.
const MAX_OPEN_READERS: usize = 200;

/// Merge multiple .versatiles containers into a single output container.
///
/// Reads a list of .versatiles containers (local paths or URLs), reads their tile indices,
/// and merges them into a single output container. Handles overlapping tiles by compositing
/// semi-transparent images using additive alpha blending.
///
/// Tiles from containers listed earlier in the input file overlay tiles from containers listed later.
#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Merge {
	/// Text file listing container paths or URLs, one per line.
	/// Empty lines and # comments are skipped. Whitespace is trimmed.
	/// Containers listed earlier overlay containers listed later.
	input_list: String,

	/// Output merged .versatiles container path.
	output: String,

	/// Lossy WebP quality for the final output tiles, using zoom-dependent syntax
	/// (e.g. "80,70,14:50,15:20"). Default: 75.
	#[arg(long, value_name = "str", default_value = "75")]
	quality: String,

	/// Encode translucent tiles as lossless WebP instead of using the lossy --quality setting
	#[arg(long)]
	lossless: bool,
}

/// Parse a quality string using the same syntax as raster_format.
fn parse_quality(quality: &str) -> Result<[Option<u8>; 32]> {
	let mut result: [Option<u8>; 32] = [None; 32];
	let mut zoom: i32 = -1;
	for part in quality.split(',') {
		let mut part = part.trim();
		zoom += 1;
		if part.is_empty() {
			continue;
		}
		if let Some(idx) = part.find(':') {
			zoom = part[0..idx].trim().parse()?;
			ensure!((0..=31).contains(&zoom), "Zoom level must be between 0 and 31");
			part = &part[(idx + 1)..];
		}
		let quality_val: u8 = part.trim().parse()?;
		ensure!(quality_val <= 100, "Quality value must be between 0 and 100");
		for z in zoom..32 {
			result[usize::try_from(z).unwrap()] = Some(quality_val);
		}
	}
	Ok(result)
}

/// Parse the input list file, returning a list of container paths/URLs.
fn parse_input_list(content: &str) -> Vec<String> {
	content
		.lines()
		.map(|line| {
			// Strip comments
			let line = if let Some(idx) = line.find('#') {
				&line[..idx]
			} else {
				line
			};
			line.trim().to_string()
		})
		.filter(|line| !line.is_empty())
		.collect()
}

pub async fn run(args: &Merge, runtime: &TilesRuntime) -> Result<()> {
	log::info!("raster merge from {:?} to {:?}", args.input_list, args.output);

	// Read and parse input list
	let list_content = std::fs::read_to_string(&args.input_list)
		.with_context(|| format!("Failed to read input list file: {}", args.input_list))?;
	let paths = parse_input_list(&list_content);
	ensure!(!paths.is_empty(), "Input list file contains no container paths");

	log::info!("merging {} containers", paths.len());

	// Scan all containers in parallel to collect metadata, then drop the readers
	let progress = runtime.create_progress("scanning containers", paths.len() as u64);
	let scan_results: Vec<Result<_>> = stream::iter(&paths)
		.map(|path| {
			let runtime = runtime.clone();
			let progress = &progress;
			async move {
				let reader = runtime
					.get_reader_from_str(path)
					.await
					.with_context(|| format!("Failed to open container: {path}"))?;
				let metadata = reader.metadata();
				let result = (
					SourceInfo {
						path: path.clone(),
						pyramid: metadata.bbox_pyramid.clone(),
					},
					metadata.tile_format,
					metadata.tile_compression,
					reader.tilejson().clone(),
					metadata.traversal.clone(),
					metadata.bbox_pyramid.clone(),
				);
				// reader is dropped here, closing the file handle
				progress.inc(1);
				Ok(result)
			}
		})
		.buffered(64)
		.collect()
		.await;
	progress.finish();

	// Merge results sequentially (order matters for source priority)
	let mut source_infos: Vec<SourceInfo> = Vec::new();
	let mut first_tile_format = None;
	let mut first_tile_compression = None;
	let mut pyramid = TileBBoxPyramid::new_empty();
	let mut tilejson = TileJSON::default();
	let mut traversal = Traversal::default();

	let progress = runtime.create_progress("merging metadata", scan_results.len() as u64);
	for result in scan_results {
		let (info, tile_format, tile_compression, tj, trav, pyr) = result?;
		if first_tile_format.is_none() {
			first_tile_format = Some(tile_format);
			first_tile_compression = Some(tile_compression);
		}
		tilejson.merge(&tj)?;
		traversal.intersect(&trav)?;
		pyramid.include_bbox_pyramid(&pyr);
		source_infos.push(info);
		progress.inc(1);
	}
	progress.finish();

	let tile_format = first_tile_format.ok_or(anyhow!("No tile format found"))?;
	let tile_compression = first_tile_compression.ok_or(anyhow!("No tile compression found"))?;

	// Parse quality
	let quality = parse_quality(&args.quality)?;

	let metadata = TileSourceMetadata::new(tile_format, tile_compression, pyramid, traversal);
	metadata.update_tilejson(&mut tilejson);

	// Create the merge source with lazy reader cache
	let cache = ReaderCache::new(source_infos, MAX_OPEN_READERS, runtime.clone());
	let merge_source = MergeSource {
		metadata,
		tilejson,
		quality,
		lossless: args.lossless,
		cache: Arc::new(Mutex::new(cache)),
	};
	let shared: SharedTileSource = merge_source.into_shared();

	let params = TilesConverterParameters::default();
	convert_tiles_container_to_str(shared, params, &args.output, runtime.clone()).await?;

	log::info!("finished raster merge");
	Ok(())
}

/// A custom TileSource that merges tiles from multiple containers.
struct MergeSource {
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
	quality: [Option<u8>; 32],
	lossless: bool,
	cache: Arc<Mutex<ReaderCache>>,
}

impl std::fmt::Debug for MergeSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MergeSource")
			.field("metadata", &self.metadata)
			.field("quality", &self.quality)
			.field("lossless", &self.lossless)
			.finish_non_exhaustive()
	}
}

#[async_trait]
impl TileSource for MergeSource {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("raster_merge", "merge")
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("raster_merge::get_tile_stream {bbox:?}");

		let quality = self.quality;
		let lossless = self.lossless;

		// Eagerly fetch and merge tiles for each sub_bbox.
		// Readers are briefly borrowed from the cache per source, then dropped,
		// so file handles don't accumulate across concurrent streams.
		let mut all_tiles: Vec<(TileCoord, Tile)> = Vec::new();
		let sub_bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(16).collect();

		for sub_bbox in sub_bboxes {
			let level = sub_bbox.level;
			let q = quality[level as usize];
			let mut result_tiles: TileBBoxMap<Option<Tile>> = TileBBoxMap::new_default(sub_bbox)?;

			// Get overlapping source indices (no readers opened yet)
			let indices: Vec<usize> = {
				let cache = self.cache.lock().await;
				cache.overlapping_sources(&bbox)
			};

			// Fetch tiles from all overlapping sources concurrently
			let cache = Arc::clone(&self.cache);
			let fetched: Vec<Result<Vec<(TileCoord, Tile)>>> = stream::iter(indices.iter().copied())
				.map(|idx| {
					let cache = Arc::clone(&cache);
					async move {
						let reader = {
							let mut cache = cache.lock().await;
							cache.get_reader(idx).await?
						};
						let mut tiles = Vec::new();
						if let Ok(tile_stream) = reader.get_tile_stream(sub_bbox).await {
							tile_stream
								.for_each(|coord, tile| {
									tiles.push((coord, tile));
								})
								.await;
						}
						// reader (Arc clone) dropped here — file closes if evicted from cache
						Ok(tiles)
					}
				})
				.buffered((1024u64 / sub_bbox.count_tiles()) as usize)
				.collect()
				.await;

			// Merge sequentially in correct order:
			// Sources listed earlier have higher priority (overlay on top)
			// We process in reverse order so earlier sources get composited on top
			for result in fetched.into_iter().rev() {
				for (coord, tile) in result? {
					let entry = result_tiles.get_mut(&coord).unwrap();
					match entry {
						None => {
							*entry = Some(tile);
						}
						Some(existing) => {
							if let Ok(merged) = merge_two_tiles(existing, &tile, q, lossless) {
								*entry = Some(merged);
							}
						}
					}
				}
			}

			all_tiles.extend(
				result_tiles
					.into_iter()
					.filter_map(|(coord, item)| item.map(|tile| (coord, tile))),
			);
		}

		Ok(TileStream::from_vec(all_tiles))
	}
}

/// Merge two tiles: `base` (bottom) and `top` (overlay on top).
/// Returns the merged tile.
fn merge_two_tiles(base: &mut Tile, top: &Tile, quality: Option<u8>, lossless: bool) -> Result<Tile> {
	// Check if top tile is opaque - if so, it completely covers the base
	let mut top_clone = top.clone();
	if top_clone.is_opaque()? {
		return Ok(top.clone());
	}

	// Both tiles need compositing
	let base_image = base.as_image()?.clone();
	let top_image = top_clone.into_image()?;

	let mut result = base_image;

	// Use additive alpha compositing
	result.overlay_additive(&top_image)?;

	// Check if result is opaque
	let is_opaque = result.is_opaque();

	// Encode as WebP
	let effective_quality = if !is_opaque && lossless { Some(100) } else { quality };

	let tile = Tile::from_image(result, TileFormat::WEBP)?;
	let mut tile = tile;
	tile.change_format(TileFormat::WEBP, effective_quality, None)?;
	Ok(tile)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_input_list() {
		let content = "
# This is a comment
tiles/001.versatiles
tiles/002.versatiles

  tiles/003.versatiles
# Another comment
https://example.com/tiles/004.versatiles
";
		let paths = parse_input_list(content);
		assert_eq!(
			paths,
			vec![
				"tiles/001.versatiles",
				"tiles/002.versatiles",
				"tiles/003.versatiles",
				"https://example.com/tiles/004.versatiles",
			]
		);
	}

	#[test]
	fn test_parse_input_list_inline_comments() {
		let content = "tiles/001.versatiles # first container\ntiles/002.versatiles";
		let paths = parse_input_list(content);
		assert_eq!(paths, vec!["tiles/001.versatiles", "tiles/002.versatiles"]);
	}

	#[test]
	fn test_parse_input_list_empty() {
		let content = "\n# only comments\n  \n";
		let paths = parse_input_list(content);
		assert!(paths.is_empty());
	}

	#[test]
	fn test_parse_quality() {
		let q = parse_quality("80").unwrap();
		assert_eq!(q[0], Some(80));
		assert_eq!(q[15], Some(80));

		let q = parse_quality("80,70,14:50,15:20").unwrap();
		assert_eq!(q[0], Some(80));
		assert_eq!(q[1], Some(70));
		assert_eq!(q[13], Some(70));
		assert_eq!(q[14], Some(50));
		assert_eq!(q[15], Some(20));
	}
}
