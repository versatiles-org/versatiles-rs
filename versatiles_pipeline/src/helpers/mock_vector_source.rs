use anyhow::Result;
use async_trait::async_trait;
use versatiles_core::{tilejson::TileJSON, types::*};
use versatiles_geometry::{
	vector_tile::{VectorTile, VectorTileLayer},
	GeoFeature, Geometry,
};

#[derive(Debug)]
pub struct MockVectorSource {
	#[allow(clippy::type_complexity)]
	data: Vec<(String, Vec<Vec<(String, String)>>)>,
	parameters: TilesReaderParameters,
	tilejson: TileJSON,
}

impl MockVectorSource {
	#[allow(clippy::type_complexity)]
	pub fn new(layers: &[(&str, &[&[(&str, &str)]])], bbox: Option<TileBBoxPyramid>) -> Self {
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
			bbox.unwrap_or_else(|| TileBBoxPyramid::new_full(8)),
		);

		let mut tilejson = TileJSON::default();
		tilejson.set_string("name", "mock vector source").unwrap();
		tilejson.update_from_reader_parameters(&parameters);

		MockVectorSource {
			data,
			parameters,
			tilejson,
		}
	}
}

#[async_trait]
impl TilesReaderTrait for MockVectorSource {
	fn get_source_name(&self) -> &str {
		"MockVectorSource"
	}

	fn get_container_name(&self) -> &str {
		"MockVectorSource"
	}

	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn override_compression(&mut self, _tile_compression: TileCompression) {
		panic!("not possible")
	}

	fn get_tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		if !self.parameters.bbox_pyramid.contains_coord(coord) {
			return Ok(None);
		}

		let mut layers = vec![];

		// Iterate over each layer and convert features
		for (name, features_def) in self.data.iter() {
			let mut features: Vec<GeoFeature> = vec![];

			// Create features for the current layer
			for properties in features_def {
				let mut feature = GeoFeature::new(Geometry::new_point([1, 2]));
				feature.set_property("x".to_string(), coord.x);
				feature.set_property("y".to_string(), coord.y);
				feature.set_property("z".to_string(), coord.z);

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
pub fn arrange_tiles<T: ToString>(tiles: Vec<(TileCoord3, Blob)>, cb: impl Fn(TileCoord3, Blob) -> T) -> Vec<String> {
	use versatiles_core::types::TileBBox;

	let mut bbox = TileBBox::new_empty(tiles.first().unwrap().0.z).unwrap();
	tiles.iter().for_each(|t| bbox.include_coord(t.0.x, t.0.y));

	let mut result: Vec<Vec<String>> = (0..bbox.height())
		.map(|_| (0..bbox.width()).map(|_| String::from("❌")).collect())
		.collect();

	for (coord, blob) in tiles.into_iter() {
		let x = (coord.x - bbox.x_min) as usize;
		let y = (coord.y - bbox.y_min) as usize;
		result[y][x] = cb(coord, blob).to_string();
	}
	result.into_iter().map(|r| r.join(" ")).collect::<Vec<String>>()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test_get_tile_data() {
		let source = MockVectorSource::new(
			&[("layer1", &[&[("key1", "value1"), ("key2", "value2")]])],
			Some(TileBBoxPyramid::from_geo_bbox(0, 8, &GeoBBox(-180.0, -90.0, 0.0, 0.0))),
		);

		assert_eq!(source.get_source_name(), "MockVectorSource");
		assert_eq!(source.get_container_name(), "MockVectorSource");
		assert!(source
			.get_parameters()
			.bbox_pyramid
			.contains_coord(&TileCoord3::new(0, 200, 8).unwrap()));

		let coord = TileCoord3::new(0, 150, 8).unwrap();
		let tile_data = source.get_tile_data(&coord).await.unwrap();

		assert!(tile_data.is_some());

		let coord = TileCoord3::new(100, 100, 8).unwrap();
		let tile_data = source.get_tile_data(&coord).await.unwrap();

		assert!(tile_data.is_none());
	}

	#[test]
	fn test_arrange_tiles() {
		let tiles = vec![
			(TileCoord3::new(0, 0, 8).unwrap(), Blob::from("a")),
			(TileCoord3::new(1, 0, 8).unwrap(), Blob::from("b")),
			(TileCoord3::new(0, 1, 8).unwrap(), Blob::from("c")),
		];

		let arranged = arrange_tiles(tiles, |_coord, blob| blob.as_str().to_string());
		assert_eq!(arranged, ["a b", "c ❌"]);
	}

	#[test]
	fn test_mock_vector_source_get_tilejson() {
		let source = MockVectorSource::new(
			&[("layer1", &[&[("key1", "value1")]])],
			Some(TileBBoxPyramid::from_geo_bbox(3, 15, &GeoBBox(-180.0, -90.0, 0.0, 0.0))),
		);
		assert_eq!(
			source.get_tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 0, 0 ],",
				"  \"maxzoom\": 15,",
				"  \"minzoom\": 3,",
				"  \"name\": \"mock vector source\",",
				"  \"tile_content\": \"vector\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);
	}
}
