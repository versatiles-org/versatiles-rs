use napi_derive::napi;
use std::collections::BTreeMap;

#[napi(object)]
#[derive(Debug, PartialEq)]
pub struct VectorLayer {
	pub id: String,
	pub fields: BTreeMap<String, String>,
	pub description: Option<String>,
	pub minzoom: Option<f64>,
	pub maxzoom: Option<f64>,
}

#[napi(object, js_name = "TileJSON")]
#[derive(Debug, PartialEq)]
pub struct TileJSON {
	pub tilejson: String,
	pub minzoom: f64,
	pub maxzoom: f64,
	/// Geographic bounding box. If `Some`, `[west, south, east, north]`.
	pub bounds: Option<Vec<f64>>,
	/// Geographic center. If `Some`, `[longitude, latitude, zoom_level]`.
	pub center: Option<Vec<f64>>,
	/// The collection of vector layers, if any.
	pub vector_layers: Option<Vec<VectorLayer>>,
	/// Optional tile content type derived from format (raster/vector/unknown).
	pub tile_type: Option<String>,
	/// Optional tile format (e.g., "image/png", "application/x-protobuf").
	pub tile_format: Option<String>,
	/// Optional tile schema describing the expected layer/attribute structure.
	pub tile_schema: Option<String>,
	/// Optional tile size in pixels (typically 256 or 512).
	pub tile_size: Option<f64>,
}

impl TileJSON {
	pub fn build(tj: &versatiles_core::TileJSON, p: &versatiles_core::TileBBoxPyramid) -> Self {
		let vector_layers = tj
			.vector_layers
			.0
			.iter()
			.map(|(id, layer)| VectorLayer {
				id: id.clone(),
				fields: layer.fields.clone(),
				description: layer.description.clone(),
				minzoom: layer.minzoom.map(|z| z as f64),
				maxzoom: layer.maxzoom.map(|z| z as f64),
			})
			.collect::<Vec<_>>();

		TileJSON {
			bounds: tj.bounds.map(|b| vec![b.x_min, b.y_min, b.x_max, b.y_max]),
			center: tj.center.map(|c| vec![c.0, c.1, c.2 as f64]),
			vector_layers: if vector_layers.is_empty() {
				None
			} else {
				Some(vector_layers)
			},
			tile_type: tj.tile_type.map(|t| t.to_string()),
			tile_format: tj.tile_format.map(|f| f.to_string()),
			tile_schema: tj.tile_schema.map(|s| s.to_string()),
			tile_size: tj.tile_size.map(|s| s.size() as f64),
			minzoom: p.get_level_min().unwrap_or(0) as f64,
			maxzoom: p.get_level_max().unwrap_or(0) as f64,
			tilejson: String::from("3.0"),
		}
	}
}
