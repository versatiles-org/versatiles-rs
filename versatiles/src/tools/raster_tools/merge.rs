use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use futures::{StreamExt, stream};
use std::sync::Arc;
use versatiles_container::{
	SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata, TilesConverterParameters, TilesRuntime,
	Traversal, convert_tiles_container_to_str,
};
use versatiles_core::{TileBBox, TileBBoxMap, TileBBoxPyramid, TileFormat, TileJSON, TileStream};
use versatiles_image::traits::{DynamicImageTraitInfo, DynamicImageTraitOperation};

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

	// Open all containers
	let mut sources: Vec<SharedTileSource> = Vec::new();
	for path in &paths {
		log::info!("opening container: {path}");
		let reader = runtime
			.get_reader_from_str(path)
			.await
			.with_context(|| format!("Failed to open container: {path}"))?;
		sources.push(reader);
	}

	// Parse quality
	let quality = parse_quality(&args.quality)?;

	// Create the merge source
	let merge_source = MergeSource::new(sources, quality, args.lossless)?;
	let shared: SharedTileSource = merge_source.into_shared();

	let params = TilesConverterParameters::default();
	convert_tiles_container_to_str(shared, params, &args.output, runtime.clone()).await?;

	log::info!("finished raster merge");
	Ok(())
}

/// A custom TileSource that merges tiles from multiple containers.
#[derive(Debug)]
struct MergeSource {
	metadata: TileSourceMetadata,
	sources: Arc<Vec<SharedTileSource>>,
	tilejson: TileJSON,
	quality: [Option<u8>; 32],
	lossless: bool,
}

impl MergeSource {
	fn new(sources: Vec<SharedTileSource>, quality: [Option<u8>; 32], lossless: bool) -> Result<Self> {
		ensure!(!sources.is_empty(), "must have at least one source");

		let first_metadata = sources[0].metadata();
		let tile_format = first_metadata.tile_format;
		let tile_compression = first_metadata.tile_compression;

		let mut pyramid = TileBBoxPyramid::new_empty();
		let mut tilejson = TileJSON::default();
		let mut traversal = Traversal::default();

		for source in &sources {
			tilejson.merge(source.tilejson())?;
			let metadata = source.metadata();
			traversal.intersect(&metadata.traversal)?;
			pyramid.include_bbox_pyramid(&metadata.bbox_pyramid);
		}

		let metadata = TileSourceMetadata::new(tile_format, tile_compression, pyramid, traversal);
		metadata.update_tilejson(&mut tilejson);

		Ok(Self {
			metadata,
			sources: Arc::new(sources),
			tilejson,
			quality,
			lossless,
		})
	}
}

#[async_trait]
impl TileSource for MergeSource {
	fn source_type(&self) -> Arc<SourceType> {
		let source_types: Vec<Arc<SourceType>> = self.sources.iter().map(|s| s.source_type()).collect();
		SourceType::new_composite("raster_merge", &source_types)
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("raster_merge::get_tile_stream {bbox:?}");

		let sources = Arc::clone(&self.sources);
		let quality = self.quality;
		let lossless = self.lossless;

		let sub_bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(32).collect();

		Ok(TileStream::from_streams(stream::iter(sub_bboxes).map(move |bbox| {
			let sources = Arc::clone(&sources);
			async move {
				let level = bbox.level;
				let q = quality[level as usize];

				let mut result_tiles: TileBBoxMap<Option<Tile>> = TileBBoxMap::new_default(bbox).unwrap();

				// Collect tiles from all sources for each coordinate
				// Sources listed earlier have higher priority (overlay on top)
				// We process in reverse order so earlier sources get composited on top
				for source in sources.iter().rev() {
					let Ok(stream) = source.get_tile_stream(bbox).await else {
						continue;
					};

					stream
						.for_each(|coord, tile| {
							let entry = result_tiles.get_mut(&coord).unwrap();
							match entry {
								None => {
									// No tile yet at this coord - just store it
									*entry = Some(tile);
								}
								Some(existing) => {
									// Composite: new tile (from earlier source) goes on top
									if let Ok(merged) = merge_two_tiles(existing, &tile, q, lossless) {
										*entry = Some(merged);
									}
								}
							}
						})
						.await;
				}

				let vec = result_tiles
					.into_iter()
					.filter_map(|(coord, item)| item.map(|tile| (coord, tile)))
					.collect::<Vec<_>>();
				TileStream::from_vec(vec)
			}
		})))
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
