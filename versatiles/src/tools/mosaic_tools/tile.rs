use anyhow::Result;
use std::collections::BTreeMap;
use versatiles_container::{TileSource, TilesConverterParameters, TilesRuntime, convert_tiles_container_to_str};
use versatiles_pipeline::{PipelineReader, VPLNode, VPLPipeline};

/// Tile a single georeferenced raster image into a tile container.
///
/// Reads a georeferenced raster (GeoTIFF, etc.) via GDAL, tiles it at a base zoom level,
/// generates overview tiles down to a minimum zoom, and writes a tile container
/// with smart WebP compression (lossy for opaque, lossless for translucent).
#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Tile {
	/// Path to a georeferenced raster image (GeoTIFF, etc.) readable by GDAL.
	input_image: String,

	/// Output .versatiles container path.
	output: String,

	/// Lowest overview zoom level to generate (default: 0).
	#[arg(long, value_name = "int", default_value = "0")]
	min_zoom: u8,

	/// Base zoom level for tiling (auto-detected from image resolution if omitted).
	#[arg(long, value_name = "int")]
	max_zoom: Option<u8>,

	/// Lossy WebP quality for opaque tiles, using zoom-dependent syntax
	/// (e.g. "70,14:50,15:20"). Default: 75.
	#[arg(long, value_name = "str", default_value = "75")]
	quality: String,

	/// Comma-separated 1-based band indices for color channels (e.g. "4,3,2" for RGB).
	/// Defaults to auto-detection from the source's color interpretation metadata.
	#[arg(long, value_name = "str")]
	bands: Option<String>,

	/// NoData value(s) to treat as transparent. Multiple values can be
	/// separated by semicolons (e.g. "0;255" treats both 0 and 255 as nodata).
	/// Each value can be a single number applied to all bands or
	/// comma-separated per-band values (e.g. "0,0,0;255,255,255").
	/// If not specified, uses the source dataset's nodata value (if any).
	#[arg(long, value_name = "str")]
	nodata: Option<String>,

	/// Override the source CRS with an EPSG code (e.g. "4326" or "25832").
	/// Use this when the input image has no embedded CRS or an incorrect one.
	#[arg(long, value_name = "EPSG")]
	crs: Option<String>,

	/// Number of concurrent GDAL instances for parallel decoding.
	/// Higher values use more memory but improve throughput for slow codecs (e.g. JPEG2000).
	/// Default: number of CPU cores.
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
