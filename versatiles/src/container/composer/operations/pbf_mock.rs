use super::{TileComposerOperation, TileComposerOperationLookup};
use crate::{
	container::{TilesReaderParameters, TilesStream},
	geometry::{
		vector_tile::{VectorTile, VectorTileLayer},
		Feature, GeoProperties, GeoValue, Geometry,
	},
	utils::YamlWrapper,
};
use anyhow::Result;
use async_trait::async_trait;
use futures::{stream, StreamExt};
use std::fmt::Debug;
use versatiles_core::types::{Blob, TileBBox, TileCompression, TileCoord3, TileFormat};

#[derive(Debug)]
pub struct PBFMock {
	blob: Blob,
	parameters: TilesReaderParameters,
}

#[async_trait]
impl TileComposerOperation for PBFMock {
	async fn new(
		_name: &str,
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
			parameters: TilesReaderParameters::new_full(TileFormat::PBF, TileCompression::None),
		})
	}

	async fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}
	async fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(Some(Blob::from("mock_meta")))
	}
	async fn get_tile_data(&self, _coord: &TileCoord3) -> Result<Option<Blob>> {
		Ok(Some(self.blob.clone()))
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TilesStream {
		let coords = bbox.iter_coords().collect::<Vec<TileCoord3>>();
		stream::iter(coords).map(|c| (c, self.blob.clone())).boxed()
	}
}
