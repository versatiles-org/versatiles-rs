use super::{TileComposerOperation, TileComposerOperationLookup};
use crate::{
	container::TilesReaderParameters,
	geometry::{
		vector_tile::{VectorTile, VectorTileLayer},
		Feature, GeoProperties, GeoValue, Geometry,
	},
	types::TileStream,
	utils::YamlWrapper,
};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_core::types::{Blob, TileBBox, TileCompression, TileCoord3, TileFormat};

#[derive(Debug)]
pub struct PBFMock {
	blob: Blob,
	name: String,
	parameters: TilesReaderParameters,
}

#[async_trait]
impl TileComposerOperation for PBFMock {
	fn get_docs() -> String {
		"mock".to_string()
	}
	async fn new(
		name: &str,
		_yaml: YamlWrapper,
		_lookup: &mut TileComposerOperationLookup,
	) -> Result<Self>
	where
		Self: Sized,
	{
		let blob = (VectorTile {
			layers: vec![VectorTileLayer::from_features(
				"test_layer".to_string(),
				vec![(1, "Bärlin"), (4, "Madrid")]
					.into_iter()
					.map(|(id, name)| Feature {
						id: None,
						geometry: Geometry::new_example(),
						properties: Some(GeoProperties::from(vec![
							("tile_id", GeoValue::from(id)),
							("tile_name", GeoValue::from(name)),
						])),
					})
					.collect(),
				4096,
				2,
			)?],
		})
		.to_blob()?;

		Ok(PBFMock {
			blob,
			name: name.to_string(),
			parameters: TilesReaderParameters::new_full(
				TileFormat::PBF,
				TileCompression::Uncompressed,
			),
		})
	}

	fn get_name(&self) -> &str {
		&self.name
	}

	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	async fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(Some(Blob::from("mock_meta")))
	}

	async fn get_tile_data(&self, _coord: &TileCoord3) -> Result<Option<Blob>> {
		Ok(Some(self.blob.clone()))
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		let coords = bbox.iter_coords().collect::<Vec<TileCoord3>>();
		TileStream::from_coord_vec_sync(coords, |c| Some((c, self.blob.clone())))
	}
}
