use anyhow::Result;
use async_trait::async_trait;
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_geometry::{
	geo::{GeoFeature, Geometry},
	vector_tile::{VectorTile, VectorTileLayer},
};

#[derive(Debug)]
pub struct DummyVectorSource {
	#[allow(clippy::type_complexity)]
	data: Vec<(String, Vec<Vec<(String, String)>>)>,
	parameters: TilesReaderParameters,
	tilejson: TileJSON,
	traversal: Traversal,
}

impl DummyVectorSource {
	#[allow(clippy::type_complexity)]
	pub fn new(layers: &[(&str, &[&[(&str, &str)]])], pyramid: Option<TileBBoxPyramid>) -> Self {
		// Convert the layers input into the required data structure
		let data: Vec<(String, Vec<Vec<(String, String)>>)> = layers
			.iter()
			.map(|(name, layer)| {
				let converted_layer = layer
					.iter()
					.map(|feature| {
						feature
							.iter()
							.map(|(key, value)| (key.to_string(), value.to_string()))
							.collect()
					})
					.collect();
				(name.to_string(), converted_layer)
			})
			.collect();

		// Initialize the parameters with the given bounding box or a default one
		let parameters = TilesReaderParameters::new(
			TileFormat::MVT,
			TileCompression::Uncompressed,
			pyramid.unwrap_or_else(|| TileBBoxPyramid::new_full(8)),
		);

		let mut tilejson = TileJSON::default();
		tilejson.set_string("name", "dummy vector source").unwrap();
		tilejson.update_from_reader_parameters(&parameters);

		DummyVectorSource {
			data,
			parameters,
			tilejson,
			traversal: Traversal::default(),
		}
	}

	#[allow(dead_code)]
	pub fn set_traversal(&mut self, traversal: Traversal) {
		self.traversal = traversal;
	}
}

#[async_trait]
impl TilesReaderTrait for DummyVectorSource {
	fn source_name(&self) -> &str {
		"DummyVectorSource"
	}

	fn container_name(&self) -> &str {
		"DummyVectorSource"
	}

	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn override_compression(&mut self, _tile_compression: TileCompression) {
		panic!("not possible")
	}

	fn traversal(&self) -> &Traversal {
		&self.traversal
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn get_tile_blob(&self, coord: &TileCoord) -> Result<Option<Blob>> {
		if !self.parameters.bbox_pyramid.contains_coord(coord) {
			return Ok(None);
		}

		let mut layers = vec![];

		// Iterate over each layer and convert features
		for (name, features_def) in self.data.iter() {
			let mut features: Vec<GeoFeature> = vec![];

			// Create features for the current layer
			for properties in features_def {
				let mut feature = GeoFeature::new(Geometry::new_point(&[1, 2]));
				feature.set_property("x".to_string(), coord.x);
				feature.set_property("y".to_string(), coord.y);
				feature.set_property("z".to_string(), coord.level);

				for (key, value) in properties {
					feature.set_property(key.to_string(), value);
				}

				features.push(feature);
			}

			// Add the layer to the layers vector
			layers.push(VectorTileLayer::from_features(name.clone(), features, 4096, 1)?);
		}

		// Create a vector tile from the layers and convert it to a blob
		Ok(Some(VectorTile::new(layers).to_blob()?))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::GeoBBox;

	#[tokio::test]
	async fn test_get_tile_blob() {
		let source = DummyVectorSource::new(
			&[("layer1", &[&[("key1", "value1"), ("key2", "value2")]])],
			Some(TileBBoxPyramid::from_geo_bbox(0, 8, &GeoBBox(-180.0, -90.0, 0.0, 0.0))),
		);

		assert_eq!(source.source_name(), "DummyVectorSource");
		assert_eq!(source.container_name(), "DummyVectorSource");
		assert!(
			source
				.parameters()
				.bbox_pyramid
				.contains_coord(&TileCoord::new(8, 0, 200).unwrap())
		);

		let coord = TileCoord::new(8, 0, 150).unwrap();
		let tile_data = source.get_tile_blob(&coord).await.unwrap();

		assert!(tile_data.is_some());

		let coord = TileCoord::new(8, 100, 100).unwrap();
		let tile_data = source.get_tile_blob(&coord).await.unwrap();

		assert!(tile_data.is_none());
	}

	#[test]
	fn test_dummy_vector_source_tilejson() {
		let source = DummyVectorSource::new(
			&[("layer1", &[&[("key1", "value1")]])],
			Some(TileBBoxPyramid::from_geo_bbox(3, 15, &GeoBBox(-180.0, -90.0, 0.0, 0.0))),
		);
		assert_eq!(
			source.tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [-180, -85.051129, 0, 0],",
				"  \"maxzoom\": 15,",
				"  \"minzoom\": 3,",
				"  \"name\": \"dummy vector source\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tile_type\": \"vector\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);
	}
}
