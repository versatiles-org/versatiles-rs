use anyhow::{Context, Result, anyhow, ensure};
use futures::{StreamExt, stream};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tar::{Builder, Header};
use versatiles_container::{Tile, TilesRuntime};
use versatiles_core::{
	Blob, TileBBoxPyramid, TileCompression, TileCoord, TileFormat, TileJSON, compression::compress, utils::HilbertIndex,
};
use versatiles_image::traits::{DynamicImageTraitInfo, DynamicImageTraitOperation};

/// Merge multiple .versatiles containers into a single output TAR.
///
/// Reads a list of .versatiles containers (local paths or URLs), reads their tile indices,
/// and merges them into a single output TAR file. Handles overlapping tiles by compositing
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

	/// Output merged .tar container path.
	output: String,

	/// Lossy WebP quality for the final output tiles, using zoom-dependent syntax
	/// (e.g. "80,70,14:50,15:20"). Default: 75.
	#[arg(long, value_name = "str", default_value = "75")]
	quality: String,

	/// Encode translucent tiles as lossless WebP instead of using the lossy --quality setting
	#[arg(long)]
	lossless: bool,

	/// Scan all sources first to collect metadata and calculate optimal processing order.
	/// Without this flag, sources are processed sequentially in file-list order.
	#[arg(long)]
	prescan: bool,
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

	ensure!(
		std::path::Path::new(&args.output)
			.extension()
			.is_some_and(|ext| ext.eq_ignore_ascii_case("tar")),
		"Output path must have .tar extension (use `versatiles convert` to convert to other formats afterward)"
	);

	let list_content = std::fs::read_to_string(&args.input_list)
		.with_context(|| format!("Failed to read input list file: {}", args.input_list))?;
	let paths = parse_input_list(&list_content);
	ensure!(!paths.is_empty(), "Input list file contains no container paths");

	let quality = parse_quality(&args.quality)?;

	log::info!("merging {} containers", paths.len());

	if args.prescan {
		run_with_prescan(&args.output, &paths, &quality, args.lossless, runtime).await
	} else {
		merge_to_tar(&args.output, &paths, &quality, args.lossless, runtime).await
	}
}

/// Prescan all sources in parallel, then merge using collected metadata.
async fn run_with_prescan(
	output: &str,
	paths: &[String],
	quality: &[Option<u8>; 32],
	lossless: bool,
	runtime: &TilesRuntime,
) -> Result<()> {
	let progress = runtime.create_progress("scanning containers", paths.len() as u64);
	let scan_results: Vec<Result<_>> = stream::iter(paths)
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
					path.clone(),
					metadata.bbox_pyramid.clone(),
					metadata.tile_format,
					metadata.tile_compression,
					reader.tilejson().clone(),
				);
				progress.inc(1);
				Ok(result)
			}
		})
		.buffered(64)
		.collect()
		.await;
	progress.finish();

	let mut prescanned: Vec<(String, TileBBoxPyramid)> = Vec::new();
	let mut first_tile_format = None;
	let mut first_tile_compression = None;
	let mut tilejson = TileJSON::default();

	for result in scan_results {
		let (path, pyramid, tile_format, tile_compression, tj) = result?;
		if first_tile_format.is_none() {
			first_tile_format = Some(tile_format);
			first_tile_compression = Some(tile_compression);
		}
		tilejson.merge(&tj)?;
		prescanned.push((path, pyramid));
	}

	let tile_format = first_tile_format.ok_or(anyhow!("No tile format found"))?;
	let tile_compression = first_tile_compression.ok_or(anyhow!("No tile compression found"))?;

	let extension_format = tile_format.as_extension();
	let extension_compression = tile_compression.as_extension();

	let file = File::create(output).with_context(|| format!("Failed to create output file: {output}"))?;
	let mut builder = Builder::new(file);

	// Write tiles.json upfront (we have all metadata from prescan)
	write_tilejson_to_tar(&mut builder, &tilejson, tile_compression, extension_compression)?;

	let mut done: HashSet<u64> = HashSet::new();
	let mut translucent_buffer: HashMap<u64, (TileCoord, Tile)> = HashMap::new();

	let progress = runtime.create_progress("merging tiles", prescanned.len() as u64);

	for (path, pyramid) in &prescanned {
		let reader = runtime
			.get_reader_from_str(path)
			.await
			.with_context(|| format!("Failed to open container: {path}"))?;

		for level_bbox in pyramid.iter_levels() {
			process_tile_stream(
				&reader,
				*level_bbox,
				&mut builder,
				&mut done,
				&mut translucent_buffer,
				quality,
				lossless,
				tile_compression,
				extension_format,
				extension_compression,
			)
			.await?;
		}
		progress.inc(1);
	}
	progress.finish();

	flush_translucent_tiles(
		&mut builder,
		translucent_buffer,
		tile_compression,
		extension_format,
		extension_compression,
		runtime,
	)?;

	builder.finish()?;
	log::info!("finished raster merge");
	Ok(())
}

/// Default merge: process sources sequentially in file-list order, no pre-scan.
async fn merge_to_tar(
	output: &str,
	paths: &[String],
	quality: &[Option<u8>; 32],
	lossless: bool,
	runtime: &TilesRuntime,
) -> Result<()> {
	let file = File::create(output).with_context(|| format!("Failed to create output file: {output}"))?;
	let mut builder = Builder::new(file);

	let mut tile_format: Option<TileFormat> = None;
	let mut tile_compression: Option<TileCompression> = None;
	let mut tilejson = TileJSON::default();
	let mut done: HashSet<u64> = HashSet::new();
	let mut translucent_buffer: HashMap<u64, (TileCoord, Tile)> = HashMap::new();

	let progress = runtime.create_progress("merging tiles", paths.len() as u64);

	for path in paths {
		let reader = runtime
			.get_reader_from_str(path)
			.await
			.with_context(|| format!("Failed to open container: {path}"))?;

		let metadata = reader.metadata();
		if tile_format.is_none() {
			tile_format = Some(metadata.tile_format);
			tile_compression = Some(metadata.tile_compression);
		}
		tilejson.merge(reader.tilejson())?;

		let tf = tile_format.unwrap();
		let tc = tile_compression.unwrap();

		for level_bbox in metadata.bbox_pyramid.iter_levels() {
			process_tile_stream(
				&reader,
				*level_bbox,
				&mut builder,
				&mut done,
				&mut translucent_buffer,
				quality,
				lossless,
				tc,
				tf.as_extension(),
				tc.as_extension(),
			)
			.await?;
		}
		// reader dropped here — file handle closed
		progress.inc(1);
	}
	progress.finish();

	let tc = tile_compression.ok_or(anyhow!("No tile format found — input list may be empty"))?;

	flush_translucent_tiles(
		&mut builder,
		translucent_buffer,
		tc,
		tile_format.unwrap().as_extension(),
		tc.as_extension(),
		runtime,
	)?;

	// Write tiles.json at the end (metadata was accumulated during merge)
	write_tilejson_to_tar(&mut builder, &tilejson, tc, tc.as_extension())?;

	builder.finish()?;
	log::info!("finished raster merge");
	Ok(())
}

/// Process all tiles from a single level bbox of a source reader.
#[allow(clippy::too_many_arguments)]
async fn process_tile_stream<W: Write>(
	reader: &versatiles_container::SharedTileSource,
	level_bbox: versatiles_core::TileBBox,
	builder: &mut Builder<W>,
	done: &mut HashSet<u64>,
	translucent_buffer: &mut HashMap<u64, (TileCoord, Tile)>,
	quality: &[Option<u8>; 32],
	lossless: bool,
	tile_compression: TileCompression,
	extension_format: &str,
	extension_compression: &str,
) -> Result<()> {
	let tile_stream = reader.get_tile_stream(level_bbox).await?;
	let mut tiles: Vec<(TileCoord, Tile)> = Vec::new();
	tile_stream
		.for_each(|coord, tile| {
			tiles.push((coord, tile));
		})
		.await;

	for (coord, mut tile) in tiles {
		let key = coord.get_hilbert_index()?;

		if done.contains(&key) {
			continue;
		}

		if let Some((_, existing)) = translucent_buffer.remove(&key) {
			// existing is higher priority (top), tile is lower priority (base)
			match merge_two_tiles(tile, existing, quality[coord.level as usize], lossless) {
				Ok((merged, is_opaque)) => {
					if is_opaque {
						write_tile_to_tar(
							builder,
							&coord,
							merged,
							tile_compression,
							extension_format,
							extension_compression,
						)?;
						done.insert(key);
					} else {
						translucent_buffer.insert(key, (coord, merged));
					}
				}
				Err(e) => {
					log::warn!("Failed to merge tile at {coord:?}: {e}");
				}
			}
		} else if tile.is_opaque().unwrap_or(false) {
			write_tile_to_tar(
				builder,
				&coord,
				tile,
				tile_compression,
				extension_format,
				extension_compression,
			)?;
			done.insert(key);
		} else {
			translucent_buffer.insert(key, (coord, tile));
		}
	}
	Ok(())
}

/// Flush remaining translucent tiles to the TAR.
fn flush_translucent_tiles<W: Write>(
	builder: &mut Builder<W>,
	translucent_buffer: HashMap<u64, (TileCoord, Tile)>,
	tile_compression: TileCompression,
	extension_format: &str,
	extension_compression: &str,
	runtime: &TilesRuntime,
) -> Result<()> {
	if translucent_buffer.is_empty() {
		return Ok(());
	}
	let progress = runtime.create_progress("flushing translucent tiles", translucent_buffer.len() as u64);
	for (_, (coord, tile)) in translucent_buffer {
		write_tile_to_tar(
			builder,
			&coord,
			tile,
			tile_compression,
			extension_format,
			extension_compression,
		)?;
		progress.inc(1);
	}
	progress.finish();
	Ok(())
}

/// Write the tiles.json metadata entry to the TAR.
fn write_tilejson_to_tar<W: Write>(
	builder: &mut Builder<W>,
	tilejson: &TileJSON,
	tile_compression: TileCompression,
	extension_compression: &str,
) -> Result<()> {
	let meta_blob = compress(Blob::from(tilejson), tile_compression)?;
	let filename = format!("tiles.json{extension_compression}");
	let mut header = Header::new_gnu();
	header.set_size(meta_blob.len());
	header.set_mode(0o644);
	builder.append_data(&mut header, Path::new(&filename), meta_blob.as_slice())?;
	Ok(())
}

/// Write a single tile entry to the TAR archive.
fn write_tile_to_tar<W: Write>(
	builder: &mut Builder<W>,
	coord: &TileCoord,
	tile: Tile,
	tile_compression: TileCompression,
	extension_format: &str,
	extension_compression: &str,
) -> Result<()> {
	let filename = format!(
		"./{}/{}/{}{}{}",
		coord.level, coord.x, coord.y, extension_format, extension_compression
	);
	let blob = tile.into_blob(tile_compression)?;
	let mut header = Header::new_gnu();
	header.set_size(blob.len());
	header.set_mode(0o644);
	builder.append_data(&mut header, Path::new(&filename), blob.as_slice())?;
	Ok(())
}

/// Merge two tiles: `base` (bottom) and `top` (overlay on top).
/// Returns the merged tile and whether it is opaque.
fn merge_two_tiles(base: Tile, mut top: Tile, quality: Option<u8>, lossless: bool) -> Result<(Tile, bool)> {
	if top.is_opaque()? {
		return Ok((top, true));
	}

	let base_image = base.into_image()?;
	let top_image = top.into_image()?;

	let mut result = base_image;
	result.overlay_additive(&top_image)?;

	let is_opaque = result.is_opaque();
	let effective_quality = if !is_opaque && lossless { Some(100) } else { quality };

	let mut tile = Tile::from_image(result, TileFormat::WEBP)?;
	tile.change_format(TileFormat::WEBP, effective_quality, None)?;
	Ok((tile, is_opaque))
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
