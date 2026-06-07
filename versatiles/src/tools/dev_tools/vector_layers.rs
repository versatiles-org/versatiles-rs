use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::io::Write;
use versatiles_container::TilesRuntime;
use versatiles_core::{TileJSON, TileType, VectorLayer, VectorLayers};
use versatiles_geometry::geo::GeoValue;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
/// Scan all vector tiles of a container and generate a TileJSON with a valid `vector_layers` field.
///
/// Every tile is decoded and inspected to collect, per layer, the set of property fields (with
/// their value types) and the zoom range in which the layer occurs. The resulting `vector_layers`
/// array replaces whatever the source declared, and the full TileJSON is printed to stdout.
pub struct VectorLayersTool {
	/// Tile container to read (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(value_name = "INPUT_FILE", verbatim_doc_comment)]
	input: String,

	/// Only scan tiles at this zoom level. If not specified, all levels are scanned.
	#[arg(long)]
	level: Option<u8>,

	/// Pretty-print the output
	#[arg(long, default_value_t = false, short = 'p')]
	pretty: bool,
}

pub async fn run(args: &VectorLayersTool, runtime: &TilesRuntime) -> Result<()> {
	let tilejson = scan(args, runtime).await?;

	let output = if args.pretty {
		tilejson.to_pretty_lines(80).join("\n")
	} else {
		tilejson.stringify()
	};
	std::io::stdout().write_all(output.as_bytes())?;
	Ok(())
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
			GeoValue::Double(_) | GeoValue::Float(_) | GeoValue::Int(_) | GeoValue::UInt(_) => {
				Some(FieldType::Number)
			}
			GeoValue::String(_) => Some(FieldType::String),
			GeoValue::Null => None,
		}
	}
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

	fn observe_field(&mut self, key: &str, ty: Option<FieldType>) {
		match self.fields.get_mut(key) {
			// First sighting of this field.
			None => {
				self.fields.insert(key.to_owned(), ty);
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

	let total: u64 = levels.iter().map(|l| pyramid.level_ref(*l).to_bbox().count_tiles()).sum();
	let progress = runtime.create_progress("Scanning vector tiles", total);

	let mut layers: BTreeMap<String, LayerInfo> = BTreeMap::new();

	for level in &levels {
		let bbox = pyramid.level_ref(*level).to_bbox();
		if bbox.is_empty() {
			continue;
		}

		let mut stream = reader.tile_stream(bbox).await?;
		while let Some((coord, tile)) = stream.next().await {
			progress.inc(1);

			let vector_tile = match tile.into_vector() {
				Ok(vt) => vt,
				Err(e) => {
					log::warn!("skipping tile {coord:?}: {e:#}");
					continue;
				}
			};

			for layer in &vector_tile.layers {
				let info = layers.entry(layer.name.clone()).or_default();
				info.observe_zoom(coord.level);

				let pm = &layer.property_manager;
				for feature in &layer.features {
					for pair in feature.tag_ids.chunks_exact(2) {
						let key = pm.key.get(pair[0])?;
						let value = pm.val.get(pair[1])?;
						info.observe_field(key, FieldType::from_value(value));
					}
				}
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
}
