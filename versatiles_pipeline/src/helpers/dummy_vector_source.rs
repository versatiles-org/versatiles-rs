use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{TileBBox, TileBBoxPyramid, TileCompression, TileCoord, TileFormat, TileJSON, TileStream};
use versatiles_geometry::{
	geo::{GeoFeature, Geometry},
	vector_tile::{VectorTile, VectorTileLayer},
};

#[derive(Debug)]
pub struct DummyVectorSource {
	#[allow(clippy::type_complexity)]
	data: Arc<Vec<(String, Vec<Vec<(String, String)>>)>>,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
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
		let metadata = TileSourceMetadata::new(
			TileFormat::MVT,
			TileCompression::Uncompressed,
			pyramid.unwrap_or_else(|| TileBBoxPyramid::new_full_up_to(8)),
			Traversal::ANY,
		);

		let mut tilejson = TileJSON::default();
		tilejson.set_string("name", "dummy vector source").unwrap();
		metadata.update_tilejson(&mut tilejson);

		DummyVectorSource {
			data: Arc::new(data),
			metadata,
			tilejson,
		}
	}

	#[allow(dead_code)]
	pub fn set_traversal(&mut self, traversal: Traversal) {
		self.metadata.traversal = traversal;
	}
}

#[async_trait]
impl TileSource for DummyVectorSource {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("dummy vector source", "dummy")
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		if !self.metadata.bbox_pyramid.contains_coord(coord) {
			return Ok(None);
		}

		let mut layers = vec![];

		// Iterate over each layer and convert features
		for (name, features_def) in self.data.as_ref() {
			let mut features: Vec<GeoFeature> = vec![];

			// Create features for the current layer
			for properties in features_def {
				let mut feature = GeoFeature::new(Geometry::new_point(&[1, 2]));
				feature.set_property("x".to_string(), coord.x);
				feature.set_property("y".to_string(), coord.y);
				feature.set_property("z".to_string(), coord.level);

				for (key, value) in properties {
					feature.set_property(key.clone(), value.clone());
				}

				features.push(feature);
			}

			// Add the layer to the layers vector
			layers.push(VectorTileLayer::from_features(name.clone(), features, 4096, 1)?);
		}

		let vector_tile = VectorTile::new(layers);
		let tile = Tile::from_vector(vector_tile, TileFormat::MVT)?;

		// Create a vector tile from the layers and convert it to a blob
		Ok(Some(tile))
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let data = Arc::clone(&self.data);
		let bbox_pyramid = self.metadata.bbox_pyramid.clone();

		Ok(TileStream::from_bbox_parallel(bbox, move |coord| {
			if !bbox_pyramid.contains_coord(&coord) {
				return None;
			}

			let mut layers = vec![];

			for (name, features_def) in data.as_ref() {
				let mut features: Vec<GeoFeature> = vec![];

				for properties in features_def {
					let mut feature = GeoFeature::new(Geometry::new_point(&[1, 2]));
					feature.set_property("x".to_string(), coord.x);
					feature.set_property("y".to_string(), coord.y);
					feature.set_property("z".to_string(), coord.level);

					for (key, value) in properties {
						feature.set_property(key.clone(), value.clone());
					}

					features.push(feature);
				}

				layers.push(VectorTileLayer::from_features(name.clone(), features, 4096, 1).ok()?);
			}

			let vector_tile = VectorTile::new(layers);
			let tile = Tile::from_vector(vector_tile, TileFormat::MVT).ok()?;

			Some(tile)
		}))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::GeoBBox;

	#[tokio::test]
	async fn test_get_tile() {
		let source = DummyVectorSource::new(
			&[("layer1", &[&[("key1", "value1"), ("key2", "value2")]])],
			Some(TileBBoxPyramid::from_geo_bbox(
				0,
				8,
				&GeoBBox::new(-180.0, -90.0, 0.0, 0.0).unwrap(),
			)),
		);

		assert!(
			source
				.metadata()
				.bbox_pyramid
				.contains_coord(&TileCoord::new(8, 0, 200).unwrap())
		);

		let coord = TileCoord::new(8, 0, 150).unwrap();
		let tile_data = source.get_tile(&coord).await.unwrap();

		assert!(tile_data.is_some());

		let coord = TileCoord::new(8, 100, 100).unwrap();
		let tile_data = source.get_tile(&coord).await.unwrap();

		assert!(tile_data.is_none());
	}

	#[test]
	fn test_dummy_vector_source_tilejson() {
		let source = DummyVectorSource::new(
			&[("layer1", &[&[("key1", "value1")]])],
			Some(TileBBoxPyramid::from_geo_bbox(
				3,
				15,
				&GeoBBox::new(-180.0, -90.0, 0.0, 0.0).unwrap(),
			)),
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
