use anyhow::Result;
use std::collections::BTreeMap;
use versatiles_container::{TileSource, TilesConverterParameters, TilesRuntime, convert_tiles_container_to_str};
use versatiles_pipeline::{PipelineReader, VPLNode, VPLPipeline};

/// Process a single georeferenced raster image into a .versatiles container.
///
/// Reads a georeferenced raster (GeoTIFF, etc.) via GDAL, tiles it at a base zoom level,
/// generates overview tiles down to a minimum zoom, and writes a .versatiles container
/// with smart WebP compression (lossy for opaque, lossless for translucent).
#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Convert {
	/// Path to a georeferenced raster image (GeoTIFF, etc.) readable by GDAL.
	input_image: String,

	/// Output .versatiles container path.
	output: String,

	/// Base zoom level for tiling (auto-detected from image resolution if omitted).
	#[arg(long, value_name = "int")]
	max_zoom: Option<u8>,

	/// Lowest overview zoom level to generate (default: 0).
	#[arg(long, value_name = "int", default_value = "0")]
	min_zoom: u8,

	/// Lossy WebP quality for opaque tiles, using zoom-dependent syntax
	/// (e.g. "80,70,14:50,15:20"). Default: 75.
	#[arg(long, value_name = "str", default_value = "75")]
	quality: String,
}

pub async fn run(args: &Convert, runtime: &TilesRuntime) -> Result<()> {
	log::info!("raster convert from {:?} to {:?}", args.input_image, args.output);

	// Build from_gdal_raster node
	let mut gdal_props = BTreeMap::new();
	gdal_props.insert("filename".to_string(), vec![args.input_image.clone()]);
	gdal_props.insert("tile_size".to_string(), vec!["512".to_string()]);
	if let Some(max_zoom) = args.max_zoom {
		gdal_props.insert("level_max".to_string(), vec![max_zoom.to_string()]);
	}
	gdal_props.insert("level_min".to_string(), vec![args.min_zoom.to_string()]);

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

	// Build pipeline: from_gdal_raster | raster_overview | raster_format
	let pipeline = VPLPipeline::from(vec![gdal_node, overview_node, format_node]);

	// Resolve the input image directory for relative path resolution
	let input_path = std::path::Path::new(&args.input_image);
	let dir = input_path.parent().unwrap_or(std::path::Path::new("."));

	let reader = PipelineReader::from_pipeline(pipeline, "raster_convert", dir, runtime.clone()).await?;
	let source = reader.into_shared();

	let params = TilesConverterParameters::default();
	convert_tiles_container_to_str(source, params, &args.output, runtime.clone()).await?;

	log::info!("finished raster convert");
	Ok(())
}
