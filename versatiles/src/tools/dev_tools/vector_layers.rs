use anyhow::{Result, anyhow, bail};
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use versatiles_container::{SharedTileSource, Tile, TileSource, TilesRuntime};
use versatiles_core::{TileJSON, TileType, VectorLayer, VectorLayers};
use versatiles_geometry::geo::GeoValue;
use versatiles_pipeline::PipelineReader;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
/// Scan all vector tiles of a container and generate a TileJSON with a valid `vector_layers` field.
///
/// Every tile is decoded and inspected to collect, per layer, the set of property fields (with
/// their value types) and the zoom range in which the layer occurs.
///
/// Without an output file, the resulting TileJSON is printed to stdout. With an output file, the
/// input is copied to it through the `meta_update` VPL operation, which replaces its TileJSON with
/// the freshly calculated one (including the new `vector_layers`).
pub struct VectorLayersTool {
	/// Tile container to read (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(value_name = "INPUT_FILE", verbatim_doc_comment)]
	input: String,

	/// Optional output container. When given, the input is written here with the calculated
	/// TileJSON applied via `meta_update`, instead of printing the TileJSON to stdout.
	#[arg(value_name = "OUTPUT_FILE")]
	output: Option<String>,

	/// Only scan tiles at this zoom level. If not specified, all levels are scanned.
	#[arg(long)]
	level: Option<u8>,

	/// Pretty-print the output
	#[arg(long, default_value_t = false, short = 'p')]
	pretty: bool,
}

pub async fn run(args: &VectorLayersTool, runtime: &TilesRuntime) -> Result<()> {
	let tilejson = scan(args, runtime).await?;

	if let Some(output) = &args.output {
		write_via_meta_update(&args.input, &tilejson, output, runtime).await?;
	} else {
		let rendered = if args.pretty {
			tilejson.to_pretty_lines(80).join("\n")
		} else {
			tilejson.stringify()
		};
		std::io::stdout().write_all(rendered.as_bytes())?;
	}
	Ok(())
}

/// Copies `input` to `output`, applying the freshly calculated `tilejson` via the
/// `meta_update` VPL operation.
async fn write_via_meta_update(input: &str, tilejson: &TileJSON, output: &str, runtime: &TilesRuntime) -> Result<()> {
	// Embed both the input path and the TileJSON as double-quoted VPL strings.
	let vpl = format!(
		"from_container filename=\"{}\" | meta_update tilejson=\"{}\"",
		escape_vpl_string(input),
		escape_vpl_string(&tilejson.stringify()),
	);
	log::debug!("running pipeline: {vpl}");

	let dir = std::env::current_dir()?;
	let reader = PipelineReader::open_str(&vpl, &dir, runtime.clone()).await?;
	let shared: SharedTileSource = Arc::new(Box::new(reader) as Box<dyn TileSource>);

	// Fail loudly on any dropped tile rather than writing a truncated container.
	runtime.set_abort_on_error(true);
	runtime.write_to_str(shared, output).await?;
	Ok(())
}

/// Escapes a string for embedding inside a VPL double-quoted literal.
///
/// The VPL parser unescapes `\\` and `\"`, so backslashes must be doubled first.
fn escape_vpl_string(value: &str) -> String {
	value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The TileJSON field type a [`GeoValue`] maps to, per the TileJSON 3.0.0 spec.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldType {
	Boolean,
	Number,
	String,
}

impl FieldType {
	fn as_str(self) -> &'static str {
		match self {
			FieldType::Boolean => "Boolean",
			FieldType::Number => "Number",
			FieldType::String => "String",
		}
	}

	/// Classifies a property value. `null` carries no type information.
	fn from_value(value: &GeoValue) -> Option<FieldType> {
		match value {
			GeoValue::Bool(_) => Some(FieldType::Boolean),
			GeoValue::Double(_) | GeoValue::Float(_) | GeoValue::Int(_) | GeoValue::UInt(_) => Some(FieldType::Number),
			GeoValue::String(_) => Some(FieldType::String),
			GeoValue::Null => None,
		}
	}
}

/// Records one observation of `key` having type `ty` into a field-type map.
///
/// `None` for `ty` means the value was `null` (no type information).
fn merge_field(fields: &mut BTreeMap<String, Option<FieldType>>, key: &str, ty: Option<FieldType>) {
	match fields.get_mut(key) {
		// First sighting of this field.
		None => {
			fields.insert(key.to_owned(), ty);
		}
		// Field already seen as `null` only — adopt the first concrete type.
		Some(existing @ None) => *existing = ty,
		// Conflicting concrete types collapse to the most general representation.
		Some(Some(existing)) => {
			if let Some(ty) = ty
				&& *existing != ty
			{
				*existing = FieldType::String;
			}
		}
	}
}

/// Field-type observations for a single layer within a single tile.
type TileLayerFields = (String, BTreeMap<String, Option<FieldType>>);

/// Decodes one tile and collapses it into per-layer field observations.
///
/// Runs the expensive work — decompression, protobuf parsing, and field-type
/// inference — so it can be dispatched to a worker thread. Observations are already
/// deduplicated per layer, so the serial merge afterwards stays cheap.
fn summarize_tile(tile: Tile) -> Result<Vec<TileLayerFields>> {
	let vector_tile = tile.into_vector()?;
	let mut result: Vec<TileLayerFields> = Vec::with_capacity(vector_tile.layers.len());

	for layer in &vector_tile.layers {
		let pm = &layer.property_manager;
		// Classify each value-table entry once, instead of per feature occurrence.
		let value_types: Vec<Option<FieldType>> = pm.val.list.iter().map(FieldType::from_value).collect();

		let mut fields = BTreeMap::new();
		for feature in &layer.features {
			for pair in feature.tag_ids.chunks_exact(2) {
				let key = pm
					.key
					.list
					.get(pair[0] as usize)
					.ok_or_else(|| anyhow!("tag key index {} out of range", pair[0]))?;
				let ty = *value_types
					.get(pair[1] as usize)
					.ok_or_else(|| anyhow!("tag value index {} out of range", pair[1]))?;
				merge_field(&mut fields, key, ty);
			}
		}
		result.push((layer.name.clone(), fields));
	}

	Ok(result)
}

/// Accumulated information about a single layer across all scanned tiles.
#[derive(Default)]
struct LayerInfo {
	/// Field name -> observed type. `None` means the field was only ever seen as `null`.
	fields: BTreeMap<String, Option<FieldType>>,
	minzoom: Option<u8>,
	maxzoom: Option<u8>,
}

impl LayerInfo {
	fn observe_zoom(&mut self, level: u8) {
		self.minzoom = Some(self.minzoom.map_or(level, |z| z.min(level)));
		self.maxzoom = Some(self.maxzoom.map_or(level, |z| z.max(level)));
	}

	fn merge_fields(&mut self, fields: BTreeMap<String, Option<FieldType>>) {
		for (key, ty) in fields {
			merge_field(&mut self.fields, &key, ty);
		}
	}

	fn into_vector_layer(self) -> VectorLayer {
		VectorLayer {
			fields: self
				.fields
				.into_iter()
				.map(|(name, ty)| (name, ty.unwrap_or(FieldType::String).as_str().to_owned()))
				.collect(),
			description: None,
			minzoom: self.minzoom,
			maxzoom: self.maxzoom,
		}
	}
}

async fn scan(args: &VectorLayersTool, runtime: &TilesRuntime) -> Result<TileJSON> {
	let reader = runtime.reader_from_str(&args.input).await?;

	if reader.metadata().tile_format().to_type() != TileType::Vector {
		bail!(
			"input is not a vector tile source (format: {:?})",
			reader.metadata().tile_format()
		);
	}

	let pyramid = reader.tile_pyramid().await?;
	let levels: Vec<u8> = if let Some(level) = args.level {
		vec![level]
	} else {
		let min = pyramid.level_min().unwrap_or(0);
		let max = pyramid.level_max().unwrap_or(0);
		(min..=max).collect()
	};

	let total: u64 = levels
		.iter()
		.map(|l| pyramid.level_ref(*l).to_bbox().count_tiles())
		.sum();
	let progress = runtime.create_progress("Scanning vector tiles", total);

	let mut layers: BTreeMap<String, LayerInfo> = BTreeMap::new();

	for level in &levels {
		let bbox = pyramid.level_ref(*level).to_bbox();
		if bbox.is_empty() {
			continue;
		}

		// Decode and summarize tiles on a worker pool (CPU-bound: decompression +
		// protobuf parsing), then merge the compact per-tile results serially.
		let progress = progress.clone();
		let mut stream = reader.tile_stream(bbox).await?.filter_map_parallel(move |coord, tile| {
			progress.inc(1);
			match summarize_tile(tile) {
				Ok(tile_layers) => Some(tile_layers),
				Err(e) => {
					log::warn!("skipping tile {coord:?}: {e:#}");
					None
				}
			}
		});

		while let Some((coord, tile_layers)) = stream.next().await {
			for (name, fields) in tile_layers {
				let info = layers.entry(name).or_default();
				info.observe_zoom(coord.level);
				info.merge_fields(fields);
			}
		}
	}

	progress.finish();

	let vector_layers = layers
		.into_iter()
		.map(|(name, info)| (name, info.into_vector_layer()))
		.collect::<VectorLayers>();

	let mut tilejson = reader.tilejson().clone();
	tilejson.vector_layers = vector_layers;
	tilejson.update_from_pyramid(pyramid.as_ref());

	Ok(tilejson)
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles::runtime::create_test_runtime;

	#[tokio::test]
	async fn test_scan_generates_vector_layers() {
		let runtime = create_test_runtime();
		let tilejson = scan(
			&VectorLayersTool {
				input: "../testdata/berlin.mbtiles".into(),
				output: None,
				level: None,
				pretty: false,
			},
			&runtime,
		)
		.await
		.unwrap();

		// The Berlin fixture is shortbread data; spot-check a known layer survives the scan.
		let place_labels = tilejson
			.vector_layers
			.find("place_labels")
			.expect("place_labels layer should be discovered");
		assert!(!place_labels.fields.is_empty());
		assert!(place_labels.minzoom.is_some());
		assert!(place_labels.maxzoom.is_some());

		// Field types must be one of the three TileJSON-spec types.
		for ty in place_labels.fields.values() {
			assert!(matches!(ty.as_str(), "String" | "Number" | "Boolean"), "got: {ty}");
		}

		// The rendered output is valid JSON carrying the generated vector_layers.
		let output = tilejson.stringify();
		assert!(output.contains("\"vector_layers\":["));
		assert!(output.contains("\"id\":\"place_labels\""));
	}

	#[tokio::test]
	async fn test_scan_single_level() {
		let runtime = create_test_runtime();
		let tilejson = scan(
			&VectorLayersTool {
				input: "../testdata/berlin.mbtiles".into(),
				output: None,
				level: Some(14),
				pretty: false,
			},
			&runtime,
		)
		.await
		.unwrap();

		// Every discovered layer should report zoom 14 only.
		for (_, layer) in tilejson.vector_layers.iter() {
			assert_eq!(layer.minzoom, Some(14));
			assert_eq!(layer.maxzoom, Some(14));
		}
	}

	#[tokio::test]
	async fn test_write_via_meta_update() {
		let runtime = create_test_runtime();
		let temp_dir = assert_fs::TempDir::new().unwrap();
		let output = temp_dir.path().join("out.mbtiles").display().to_string();

		run(
			&VectorLayersTool {
				input: "../testdata/berlin.mbtiles".into(),
				output: Some(output.clone()),
				level: None,
				pretty: false,
			},
			&runtime,
		)
		.await
		.unwrap();

		// Re-open the written container; its TileJSON must carry the calculated vector_layers.
		let written = runtime.reader_from_str(&output).await.unwrap();
		assert!(
			written.tilejson().vector_layers.find("place_labels").is_some(),
			"written container should carry the calculated vector_layers"
		);
	}

	#[test]
	fn test_escape_vpl_string() {
		assert_eq!(escape_vpl_string("plain"), "plain");
		assert_eq!(escape_vpl_string(r#"{"a":"b"}"#), r#"{\"a\":\"b\"}"#);
		assert_eq!(escape_vpl_string(r"back\slash"), r"back\\slash");
	}
}
