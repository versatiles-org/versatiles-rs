use anyhow::Result;
use async_trait::async_trait;
use versatiles_core::types::{
	Blob, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat, TilesReaderParameters,
	TilesReaderTrait,
};
use versatiles_geometry::{
	vector_tile::{VectorTile, VectorTileLayer},
	Feature, Geometry,
};

#[derive(Debug)]
pub struct MockVectorSource {
	#[allow(clippy::type_complexity)]
	data: Vec<(String, Vec<Vec<(String, String)>>)>,
	parameters: TilesReaderParameters,
}

impl MockVectorSource {
	#[allow(clippy::type_complexity)]
	pub fn new(layers: &[(&str, &[&[(&str, &str)]])], bbox: Option<TileBBoxPyramid>) -> Self {
		// Convert the layers input into the required data structure
		let data = layers
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
			TileFormat::PBF,
			TileCompression::Uncompressed,
			bbox.unwrap_or_else(|| TileBBoxPyramid::new_full(8)),
		);

		MockVectorSource { data, parameters }
	}
}

#[async_trait]
impl TilesReaderTrait for MockVectorSource {
	fn get_name(&self) -> &str {
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

	fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(Some(Blob::from(r##"{"mock":true}"##)))
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		let mut layers = vec![];

		// Iterate over each layer and convert features
		for (name, features_def) in self.data.iter() {
			let mut features: Vec<Feature> = vec![];

			// Create features for the current layer
			for properties in features_def {
				let mut feature = Feature::new(Geometry::new_point([1, 2]));
				feature.set_property("x".to_string(), coord.x);
				feature.set_property("y".to_string(), coord.y);
				feature.set_property("z".to_string(), coord.z);

				for (key, value) in properties {
					feature.set_property(key.to_string(), value);
				}

				features.push(feature);
			}

			// Add the layer to the layers vector
			layers.push(VectorTileLayer::from_features(
				name.clone(),
				features,
				4096,
				1,
			)?);
		}

		// Create a vector tile from the layers and convert it to a blob
		Ok(Some(VectorTile::new(layers).to_blob()?))
	}
}

#[cfg(test)]
pub fn arrange_tiles<T: ToString>(
	tiles: Vec<(TileCoord3, Blob)>,
	cb: impl Fn(TileCoord3, Blob) -> T,
) -> Vec<String> {
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
	result
		.into_iter()
		.map(|r| r.join(" "))
		.collect::<Vec<String>>()
}
