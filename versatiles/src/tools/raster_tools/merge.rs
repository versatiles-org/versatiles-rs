use anyhow::{Context, Result, anyhow, ensure};
use futures::{StreamExt, stream};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tar::{Builder, Header};
use versatiles_container::{Tile, TilesRuntime};
use versatiles_core::{
	Blob, MAX_ZOOM_LEVEL, TileBBoxPyramid, TileCompression, TileCoord, TileFormat, TileJSON, compression::compress,
	utils::HilbertIndex,
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

	/// Scan all sources in parallel before merging to validate accessibility
	/// and collect metadata upfront. Without this flag, sources are opened
	/// and processed one at a time in file-list order.
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

	// Optionally prescan all sources in parallel to validate accessibility and collect pyramids
	let prescanned_pyramids = if args.prescan {
		Some(prescan_sources(&paths, runtime).await?)
	} else {
		None
	};

	merge_to_tar(
		&args.output,
		&paths,
		prescanned_pyramids.as_deref(),
		&quality,
		args.lossless,
		runtime,
	)
	.await
}

/// Scan all sources in parallel, returning their pyramids in source order.
async fn prescan_sources(paths: &[String], runtime: &TilesRuntime) -> Result<Vec<TileBBoxPyramid>> {
	let progress = runtime.create_progress("scanning containers", paths.len() as u64);
	let scan_results: Vec<Result<TileBBoxPyramid>> = stream::iter(paths)
		.map(|path| {
			let runtime = runtime.clone();
			let progress = &progress;
			async move {
				let reader = runtime
					.get_reader_from_str(path)
					.await
					.with_context(|| format!("Failed to open container: {path}"))?;
				let pyramid = reader.metadata().bbox_pyramid.clone();
				progress.inc(1);
				Ok(pyramid)
			}
		})
		.buffered(64)
		.collect()
		.await;
	progress.finish();

	scan_results.into_iter().collect()
}

const NUM_LEVELS: usize = (MAX_ZOOM_LEVEL + 1) as usize;

type SuffixMinX = Vec<[Option<u32>; NUM_LEVELS]>;

/// Build source processing order (west-to-east) and per-level suffix minimum x arrays.
fn build_sweep_order(num_sources: usize, pyramids: &[TileBBoxPyramid]) -> (Vec<usize>, SuffixMinX) {
	let mut order: Vec<usize> = (0..num_sources).collect();
	order.sort_by(|&a, &b| western_edge(&pyramids[a]).total_cmp(&western_edge(&pyramids[b])));

	let mut suffix: SuffixMinX = vec![[None; NUM_LEVELS]; order.len() + 1];
	for pos in (0..order.len()).rev() {
		let idx = order[pos];
		suffix[pos] = suffix[pos + 1];
		for level in 0..=(MAX_ZOOM_LEVEL) {
			let bbox = pyramids[idx].get_level_bbox(level);
			if !bbox.is_empty()
				&& let Ok(x) = bbox.x_min()
			{
				let l = level as usize;
				suffix[pos][l] = Some(match suffix[pos][l] {
					Some(existing) => existing.min(x),
					None => x,
				});
			}
		}
	}
	(order, suffix)
}

/// Flush translucent tiles whose x-column is no longer covered by any remaining source.
fn sweep_flush<W: Write>(
	remaining_min_x: &[Option<u32>; NUM_LEVELS],
	translucent_buffer: &Arc<Mutex<HashMap<u64, (TileCoord, Tile)>>>,
	builder: &Arc<Mutex<Builder<W>>>,
	tile_compression: TileCompression,
	ext_format: &str,
	ext_compression: &str,
) -> Result<()> {
	let mut buf = translucent_buffer.lock().unwrap();
	let flush_keys: Vec<u64> = buf
		.iter()
		.filter(|(_, (coord, _))| match remaining_min_x[coord.level as usize] {
			Some(min_x) => coord.x < min_x,
			None => true,
		})
		.map(|(&key, _)| key)
		.collect();
	if flush_keys.is_empty() {
		return Ok(());
	}
	let to_flush: Vec<_> = flush_keys
		.iter()
		.filter_map(|k| buf.remove(k).map(|v| (*k, v)))
		.collect();
	drop(buf);

	log::debug!("sweep-line flush: writing {} translucent tiles", to_flush.len());
	let mut b = builder.lock().unwrap();
	for (_, (coord, tile)) in to_flush {
		let (filename, blob) = prepare_tile_entry(&coord, tile, tile_compression, ext_format, ext_compression)?;
		append_blob_to_tar(&mut b, &filename, &blob)?;
	}
	Ok(())
}

/// Compute the normalized western edge of a pyramid as the minimum fractional x across all levels.
fn western_edge(pyramid: &TileBBoxPyramid) -> f64 {
	pyramid
		.iter_levels()
		.filter_map(|bbox| bbox.x_min().ok().map(|x| f64::from(x) / f64::from(1u32 << bbox.level)))
		.reduce(f64::min)
		.unwrap_or(0.0)
}

/// Merge sources into a TAR file. If `prescanned_pyramids` is provided, uses those
/// instead of reading pyramids from each source during the merge.
async fn merge_to_tar(
	output: &str,
	paths: &[String],
	prescanned_pyramids: Option<&[TileBBoxPyramid]>,
	quality: &[Option<u8>; 32],
	lossless: bool,
	runtime: &TilesRuntime,
) -> Result<()> {
	let file = File::create(output).with_context(|| format!("Failed to create output file: {output}"))?;
	let builder = Arc::new(Mutex::new(Builder::new(BufWriter::new(file))));
	let done: Arc<Mutex<HashSet<u64>>> = Arc::default();
	let translucent_buffer: Arc<Mutex<HashMap<u64, (TileCoord, Tile)>>> = Arc::default();

	let mut tile_format: Option<TileFormat> = None;
	let mut tile_compression: Option<TileCompression> = None;
	let mut tilejson = TileJSON::default();

	// When prescan data is available, sort sources west-to-east and precompute
	// per-level suffix minimum x so we can flush translucent tiles early.
	let (source_order, suffix_min_x): (Vec<usize>, Option<SuffixMinX>) = if let Some(pyramids) = prescanned_pyramids {
		let (order, suffix) = build_sweep_order(paths.len(), pyramids);
		(order, Some(suffix))
	} else {
		((0..paths.len()).collect(), None)
	};

	let progress = runtime.create_progress("merging tiles", paths.len() as u64);

	for (pos, &idx) in source_order.iter().enumerate() {
		let path = &paths[idx];
		let reader = runtime
			.get_reader_from_str(path)
			.await
			.with_context(|| format!("Failed to open container: {path}"))?;

		let metadata = reader.metadata();
		if let Some(tf) = tile_format {
			ensure!(
				metadata.tile_format == tf,
				"Source {path} has tile format {:?}, expected {:?}",
				metadata.tile_format,
				tf
			);
			ensure!(
				metadata.tile_compression == tile_compression.unwrap(),
				"Source {path} has tile compression {:?}, expected {:?}",
				metadata.tile_compression,
				tile_compression.unwrap()
			);
		} else {
			tile_format = Some(metadata.tile_format);
			tile_compression = Some(metadata.tile_compression);
		}
		tilejson.merge(reader.tilejson())?;

		let tf = tile_format.unwrap();
		let tc = tile_compression.unwrap();
		let ext_format = tf.as_extension().to_string();
		let ext_compression = tc.as_extension().to_string();

		let pyramid = prescanned_pyramids.map_or(&metadata.bbox_pyramid, |p| &p[idx]);

		for level_bbox in pyramid.iter_levels() {
			process_tile_stream(
				&reader,
				*level_bbox,
				&builder,
				&done,
				&translucent_buffer,
				quality,
				lossless,
				tc,
				&ext_format,
				&ext_compression,
			)
			.await?;
		}
		// reader dropped here — file handle closed

		// Sweep-line flush: remove translucent tiles that no remaining source can cover
		if let Some(ref suffix) = suffix_min_x {
			sweep_flush(
				&suffix[pos + 1],
				&translucent_buffer,
				&builder,
				tc,
				&ext_format,
				&ext_compression,
			)?;
		}

		progress.inc(1);
	}
	progress.finish();

	let tf = tile_format.ok_or(anyhow!("No sources were processed"))?;
	let tc = tile_compression.unwrap();
	let ext_format = tf.as_extension();
	let ext_compression = tc.as_extension();

	let mut builder = Arc::try_unwrap(builder)
		.map_err(|_| anyhow!("builder still has references"))?
		.into_inner()?;
	let translucent_buffer = Arc::try_unwrap(translucent_buffer)
		.map_err(|_| anyhow!("translucent_buffer still has references"))?
		.into_inner()?;

	flush_translucent_tiles(
		&mut builder,
		translucent_buffer,
		tc,
		ext_format,
		ext_compression,
		runtime,
	)?;

	// Write tiles.json at the end (metadata was accumulated during merge)
	write_tilejson_to_tar(&mut builder, &tilejson, tc, ext_compression)?;

	builder.finish()?;
	log::info!("finished raster merge");
	Ok(())
}

/// Process all tiles from a single level bbox of a source reader.
#[allow(clippy::too_many_arguments)]
async fn process_tile_stream<W: Write + Send + 'static>(
	reader: &versatiles_container::SharedTileSource,
	level_bbox: versatiles_core::TileBBox,
	builder: &Arc<Mutex<Builder<W>>>,
	done: &Arc<Mutex<HashSet<u64>>>,
	translucent_buffer: &Arc<Mutex<HashMap<u64, (TileCoord, Tile)>>>,
	quality: &[Option<u8>; 32],
	lossless: bool,
	tile_compression: TileCompression,
	extension_format: &str,
	extension_compression: &str,
) -> Result<()> {
	let tile_stream = reader.get_tile_stream(level_bbox).await?;

	let builder = Arc::clone(builder);
	let done = Arc::clone(done);
	let translucent_buffer = Arc::clone(translucent_buffer);
	let quality = *quality;
	let extension_format = extension_format.to_string();
	let extension_compression = extension_compression.to_string();

	tile_stream
		.for_each_parallel_try(move |coord, mut tile| {
			let key = coord.get_hilbert_index()?;

			if done.lock().unwrap().contains(&key) {
				return Ok(());
			}

			let existing = translucent_buffer.lock().unwrap().remove(&key);

			if let Some((_, existing)) = existing {
				// existing is higher priority (top), tile is lower priority (base)
				match merge_two_tiles(tile, existing, quality[coord.level as usize], lossless) {
					Ok((merged, is_opaque)) => {
						if is_opaque {
							let (filename, blob) = prepare_tile_entry(
								&coord,
								merged,
								tile_compression,
								&extension_format,
								&extension_compression,
							)?;
							append_blob_to_tar(&mut builder.lock().unwrap(), &filename, &blob)?;
							done.lock().unwrap().insert(key);
						} else {
							translucent_buffer.lock().unwrap().insert(key, (coord, merged));
						}
					}
					Err(e) => {
						log::warn!("Failed to merge tile at {coord:?}: {e}");
					}
				}
			} else if tile.is_opaque().unwrap_or(false) {
				let (filename, blob) = prepare_tile_entry(
					&coord,
					tile,
					tile_compression,
					&extension_format,
					&extension_compression,
				)?;
				append_blob_to_tar(&mut builder.lock().unwrap(), &filename, &blob)?;
				done.lock().unwrap().insert(key);
			} else {
				translucent_buffer.lock().unwrap().insert(key, (coord, tile));
			}

			Ok(())
		})
		.await?;
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
		let (filename, blob) =
			prepare_tile_entry(&coord, tile, tile_compression, extension_format, extension_compression)?;
		append_blob_to_tar(builder, &filename, &blob)?;
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

/// Prepare a tile for writing: compress into a blob and build the filename.
/// This is the CPU-heavy part and should be called without holding any locks.
fn prepare_tile_entry(
	coord: &TileCoord,
	tile: Tile,
	tile_compression: TileCompression,
	extension_format: &str,
	extension_compression: &str,
) -> Result<(String, Blob)> {
	let filename = format!(
		"./{}/{}/{}{}{}",
		coord.level, coord.x, coord.y, extension_format, extension_compression
	);
	let blob = tile.into_blob(tile_compression)?;
	Ok((filename, blob))
}

/// Append a prepared blob to the TAR archive. This is brief I/O only.
fn append_blob_to_tar<W: Write>(builder: &mut Builder<W>, filename: &str, blob: &Blob) -> Result<()> {
	let mut header = Header::new_gnu();
	header.set_size(blob.len());
	header.set_mode(0o644);
	builder.append_data(&mut header, Path::new(filename), blob.as_slice())?;
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
