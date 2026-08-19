//! Small TileJSON helpers shared by sources that emit a single vector layer.

use std::collections::BTreeMap;

use versatiles_core::{TileJSON, VectorLayer, VectorLayers};

/// Declare `layer_name` as the source's only vector layer.
///
/// Vector consumers like QGIS need the TileJSON `vector_layers` entry to know
/// what is in each MVT layer, so every single-layer source has to publish one.
pub fn set_single_vector_layer(
	tilejson: &mut TileJSON,
	layer_name: &str,
	fields: BTreeMap<String, String>,
	minzoom: u8,
	maxzoom: u8,
) {
	let layer = VectorLayer {
		fields,
		description: None,
		minzoom: Some(minzoom),
		maxzoom: Some(maxzoom),
	};
	tilejson.vector_layers = VectorLayers(std::iter::once((layer_name.to_string(), layer)).collect());
}
