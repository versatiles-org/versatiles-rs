use anyhow::Result;
use std::collections::BTreeMap;
use versatiles_container::{TileSource, TilesConverterParameters, TilesRuntime, convert_tiles_container_to_str};
use versatiles_pipeline::{PipelineReader, VPLNode, VPLPipeline};

#[derive(clap::Args, Debug)]
#[command(
	arg_required_else_help = true,
	disable_version_flag = true,
	about = "Tile a georeferenced raster into a .versatiles container",
	long_about = "\
Tile a georeferenced raster into a .versatiles container.

Reprojects the input to Web Mercator (EPSG:3857) via GDAL, slices it at a
native \"base\" zoom level, and builds overview tiles down to a minimum zoom.
Output tiles are encoded as WebP: lossy for fully-opaque tiles (controlled
by --quality) and lossless for tiles with any transparency.

Base zoom is --max-zoom. It is the finest zoom level written — the one at
which the input pixel grid is sampled most directly. Overviews are
progressively coarser zooms produced by averaging.

EXAMPLES

  # Auto-detect base zoom from image resolution; build overviews from z=0.
  versatiles mosaic tile scene.tif scene.versatiles

  # Explicit base zoom; skip overviews below z=6 (smaller output).
  versatiles mosaic tile --min-zoom 6 --max-zoom 14 scene.tif scene.versatiles

  # 4-band image (R,G,B,NIR): use bands 1-3 for color; treat 0 as nodata.
  versatiles mosaic tile --bands 1,2,3 --nodata 0 scene.tif scene.versatiles

  # Image is in UTM zone 32N but lacks a CRS tag — assert it explicitly.
  versatiles mosaic tile --crs 25832 scene.tif scene.versatiles"
)]
pub struct Tile {
	/// Georeferenced raster readable by GDAL (GeoTIFF, JPEG2000, COG, ...).
	/// Must carry either a valid CRS/geotransform or be combined with --crs.
	input_image: String,

	/// Output .versatiles container. Overwrites any existing file.
	output: String,

	/// Coarsest overview zoom level to write. Use a higher value to drop
	/// low-resolution overviews and shrink the output.
	#[arg(long, value_name = "int", default_value = "0")]
	min_zoom: u8,

	/// Finest (base) zoom level to tile the image at.
	///
	/// Chosen automatically from the image's ground resolution if omitted.
	/// Every overview zoom between --min-zoom and this value is generated.
	#[arg(long, value_name = "int")]
	max_zoom: Option<u8>,

	/// Lossy WebP quality (0-100) for opaque tiles.
	///
	/// A single number (e.g. "75") applies to every zoom. A comma-separated
	/// list ramps quality from z=0 upwards: "90,80,70" means z=0 → 90,
	/// z=1 → 80, z≥2 → 70 (the last value extends to all higher zooms).
	/// Use "Z:Q" to jump to zoom Z explicitly, e.g. "70,14:50,15:20" means
	/// z<14 → 70, z=14 → 50, z≥15 → 20. Translucent tiles ignore this and
	/// are encoded losslessly.
	#[arg(long, value_name = "str", default_value = "75")]
	quality: String,

	/// 1-based band indices to map onto R, G, B (and optionally A).
	///
	/// Example: "4,3,2" uses band 4 as red, band 3 as green, band 2 as blue —
	/// the standard false-color composite for Landsat. Auto-detected from
	/// the source's color interpretation metadata if omitted.
	#[arg(long, value_name = "str")]
	bands: Option<String>,

	/// Pixel values to treat as transparent.
	///
	/// Syntax: one or more values separated by semicolons. Each value may be
	/// a single number (applied to every band) or comma-separated per-band
	/// values. Examples: "0" (treat 0 in every band as nodata),
	/// "0;255" (both 0 and 255 are nodata), "0,0,0;255,255,255" (pure black
	/// and pure white pixels are transparent). Defaults to the source
	/// dataset's embedded nodata value, if any.
	#[arg(long, value_name = "str")]
	nodata: Option<String>,

	/// Assert the source CRS as an EPSG code (e.g. "4326", "3857", "25832").
	///
	/// Overrides whatever the file declares — use only when the input lacks
	/// a CRS or has an incorrect one. The reprojection target is always
	/// Web Mercator (EPSG:3857) and is not configurable.
	#[arg(long, value_name = "EPSG")]
	crs: Option<String>,

	/// Maximum number of concurrent GDAL readers.
	///
	/// Raise this to speed up decoding of slow codecs (e.g. JPEG2000) at the
	/// cost of more memory. Default: one reader per CPU core.
	#[arg(long, value_name = "int")]
	gdal_concurrency: Option<u8>,
}

pub async fn run(args: &Tile, runtime: &TilesRuntime) -> Result<()> {
	log::info!("mosaic tile from {:?} to {:?}", args.input_image, args.output);

	let pipeline = build_pipeline(args);

	// Resolve the input image directory for relative path resolution
	let input_path = std::path::Path::new(&args.input_image);
	let dir = input_path.parent().unwrap_or(std::path::Path::new("."));

	let reader = PipelineReader::from_pipeline(pipeline, "mosaic_tile", dir, runtime.clone()).await?;
	let source = reader.into_shared();

	let params = TilesConverterParameters::default();
	convert_tiles_container_to_str(source, params, &args.output, runtime.clone()).await?;

	log::info!("finished mosaic tile");
	Ok(())
}

/// Build the VPL pipeline from CLI arguments.
///
/// Constructs three nodes: `from_gdal_raster | raster_overview | raster_format`.
fn build_pipeline(args: &Tile) -> VPLPipeline {
	// Build from_gdal_raster node
	let mut gdal_props = BTreeMap::new();
	gdal_props.insert("filename".to_string(), vec![args.input_image.clone()]);
	gdal_props.insert("tile_size".to_string(), vec!["512".to_string()]);
	if let Some(max_zoom) = args.max_zoom {
		gdal_props.insert("level_max".to_string(), vec![max_zoom.to_string()]);
	}
	gdal_props.insert("level_min".to_string(), vec![args.min_zoom.to_string()]);
	if let Some(ref bands) = args.bands {
		gdal_props.insert("bands".to_string(), vec![bands.clone()]);
	}
	if let Some(ref nodata) = args.nodata {
		gdal_props.insert("nodata".to_string(), vec![nodata.clone()]);
	}
	if let Some(ref crs) = args.crs {
		gdal_props.insert("crs".to_string(), vec![crs.clone()]);
	}
	if let Some(concurrency) = args.gdal_concurrency {
		gdal_props.insert("gdal_concurrency_limit".to_string(), vec![concurrency.to_string()]);
	}

	let gdal_node = VPLNode {
		name: "from_gdal_raster".to_string(),
		properties: gdal_props,
		sources: vec![],
	};

	// Build raster_overview node
	let overview_node = VPLNode {
		name: "raster_overview".to_string(),
		properties: BTreeMap::new(),
		sources: vec![],
	};

	// Build raster_format node
	let mut format_props = BTreeMap::new();
	format_props.insert("format".to_string(), vec!["webp".to_string()]);
	format_props.insert("quality".to_string(), vec![args.quality.clone()]);
	format_props.insert("quality_translucent".to_string(), vec!["100".to_string()]);

	let format_node = VPLNode {
		name: "raster_format".to_string(),
		properties: format_props,
		sources: vec![],
	};

	VPLPipeline::from(vec![gdal_node, overview_node, format_node])
}

#[cfg(test)]
mod tests {
	use super::*;

	fn default_args() -> Tile {
		Tile {
			input_image: "/path/to/image.tif".to_string(),
			output: "/path/to/output.versatiles".to_string(),
			min_zoom: 0,
			max_zoom: None,
			quality: "75".to_string(),
			bands: None,
			nodata: None,
			crs: None,
			gdal_concurrency: None,
		}
	}

	fn get_prop(node: &VPLNode, key: &str) -> Option<String> {
		node.properties.get(key).map(|v| v[0].clone())
	}

	#[test]
	fn pipeline_has_three_nodes() {
		let pipeline = build_pipeline(&default_args());
		assert_eq!(pipeline.pipeline.len(), 3);
		assert_eq!(pipeline.pipeline[0].name, "from_gdal_raster");
		assert_eq!(pipeline.pipeline[1].name, "raster_overview");
		assert_eq!(pipeline.pipeline[2].name, "raster_format");
	}

	#[test]
	fn gdal_node_has_required_props() {
		let pipeline = build_pipeline(&default_args());
		let gdal = &pipeline.pipeline[0];
		assert_eq!(get_prop(gdal, "filename").unwrap(), "/path/to/image.tif");
		assert_eq!(get_prop(gdal, "tile_size").unwrap(), "512");
		assert_eq!(get_prop(gdal, "level_min").unwrap(), "0");
		assert!(!gdal.properties.contains_key("level_max"));
	}

	#[test]
	fn gdal_node_with_max_zoom() {
		let args = Tile {
			max_zoom: Some(14),
			..default_args()
		};
		let pipeline = build_pipeline(&args);
		let gdal = &pipeline.pipeline[0];
		assert_eq!(get_prop(gdal, "level_max").unwrap(), "14");
	}

	#[test]
	fn gdal_node_with_min_zoom() {
		let args = Tile {
			min_zoom: 5,
			..default_args()
		};
		let pipeline = build_pipeline(&args);
		let gdal = &pipeline.pipeline[0];
		assert_eq!(get_prop(gdal, "level_min").unwrap(), "5");
	}

	#[test]
	fn gdal_node_with_bands() {
		let args = Tile {
			bands: Some("4,3,2".to_string()),
			..default_args()
		};
		let pipeline = build_pipeline(&args);
		let gdal = &pipeline.pipeline[0];
		assert_eq!(get_prop(gdal, "bands").unwrap(), "4,3,2");
	}

	#[test]
	fn gdal_node_with_nodata() {
		let args = Tile {
			nodata: Some("0;255".to_string()),
			..default_args()
		};
		let pipeline = build_pipeline(&args);
		let gdal = &pipeline.pipeline[0];
		assert_eq!(get_prop(gdal, "nodata").unwrap(), "0;255");
	}

	#[test]
	fn gdal_node_with_crs() {
		let args = Tile {
			crs: Some("25832".to_string()),
			..default_args()
		};
		let pipeline = build_pipeline(&args);
		let gdal = &pipeline.pipeline[0];
		assert_eq!(get_prop(gdal, "crs").unwrap(), "25832");
	}

	#[test]
	fn gdal_node_with_concurrency() {
		let args = Tile {
			gdal_concurrency: Some(4),
			..default_args()
		};
		let pipeline = build_pipeline(&args);
		let gdal = &pipeline.pipeline[0];
		assert_eq!(get_prop(gdal, "gdal_concurrency_limit").unwrap(), "4");
	}

	#[test]
	fn gdal_node_without_optional_props() {
		let pipeline = build_pipeline(&default_args());
		let gdal = &pipeline.pipeline[0];
		assert!(!gdal.properties.contains_key("level_max"));
		assert!(!gdal.properties.contains_key("bands"));
		assert!(!gdal.properties.contains_key("nodata"));
		assert!(!gdal.properties.contains_key("crs"));
		assert!(!gdal.properties.contains_key("gdal_concurrency_limit"));
	}

	#[test]
	fn overview_node_has_no_props() {
		let pipeline = build_pipeline(&default_args());
		let overview = &pipeline.pipeline[1];
		assert!(overview.properties.is_empty());
		assert!(overview.sources.is_empty());
	}

	#[test]
	fn format_node_has_webp_config() {
		let pipeline = build_pipeline(&default_args());
		let format = &pipeline.pipeline[2];
		assert_eq!(get_prop(format, "format").unwrap(), "webp");
		assert_eq!(get_prop(format, "quality").unwrap(), "75");
		assert_eq!(get_prop(format, "quality_translucent").unwrap(), "100");
	}

	#[test]
	fn format_node_with_custom_quality() {
		let args = Tile {
			quality: "70,14:50,15:20".to_string(),
			..default_args()
		};
		let pipeline = build_pipeline(&args);
		let format = &pipeline.pipeline[2];
		assert_eq!(get_prop(format, "quality").unwrap(), "70,14:50,15:20");
	}

	#[test]
	fn all_options_set() {
		let args = Tile {
			input_image: "/data/ortho.tif".to_string(),
			output: "out.versatiles".to_string(),
			min_zoom: 3,
			max_zoom: Some(18),
			quality: "80".to_string(),
			bands: Some("1,2,3".to_string()),
			nodata: Some("0,0,0".to_string()),
			crs: Some("4326".to_string()),
			gdal_concurrency: Some(8),
		};
		let pipeline = build_pipeline(&args);
		let gdal = &pipeline.pipeline[0];

		assert_eq!(get_prop(gdal, "filename").unwrap(), "/data/ortho.tif");
		assert_eq!(get_prop(gdal, "tile_size").unwrap(), "512");
		assert_eq!(get_prop(gdal, "level_min").unwrap(), "3");
		assert_eq!(get_prop(gdal, "level_max").unwrap(), "18");
		assert_eq!(get_prop(gdal, "bands").unwrap(), "1,2,3");
		assert_eq!(get_prop(gdal, "nodata").unwrap(), "0,0,0");
		assert_eq!(get_prop(gdal, "crs").unwrap(), "4326");
		assert_eq!(get_prop(gdal, "gdal_concurrency_limit").unwrap(), "8");

		let format = &pipeline.pipeline[2];
		assert_eq!(get_prop(format, "quality").unwrap(), "80");
	}

	#[test]
	fn all_nodes_have_empty_sources() {
		let pipeline = build_pipeline(&default_args());
		for node in &pipeline.pipeline {
			assert!(node.sources.is_empty());
		}
	}
}
