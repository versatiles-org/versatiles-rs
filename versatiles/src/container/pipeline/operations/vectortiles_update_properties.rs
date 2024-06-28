use crate::{
	container::{
		pipeline::{read_csv_file, OperationKDLEnum, OperationTrait},
		Factory, TilesReaderParameters,
	},
	geometry::{vector_tile::VectorTile, GeoProperties},
	types::Blob,
	utils::{decompress, kdl::KDLNode},
};
use anyhow::{anyhow, ensure, Context, Result};
use async_trait::async_trait;
use futures::future::BoxFuture;
use log::warn;
use std::{collections::HashMap, sync::Arc};
use versatiles_core::types::{TileBBox, TileCompression, TileCoord3, TileFormat, TileStream};

#[derive(versatiles_derive::KDLDecode, Clone, Debug)]
/// This operation loads a data source (like a CSV file).
/// For each feature in the vector tiles, it uses the id to fetch the correct row in the data source and uses this row to update the properties of the feature.
pub struct Args {
	/// Path of the data source, e.g., data.csv
	data_source_path: String,
	/// Field name of the id in the vector tiles
	id_field_tiles: String,
	/// Field name of the id in the data source
	id_field_values: String,
	/// Name of the layer in which properties should be replaced. If not set, properties in all layers will be replaced.
	layer_name: Option<String>,
	/// By default, the old properties in the tiles are updated with the new ones. Set "replace_properties" if the properties should be replaced with the new ones.
	replace_properties: bool,
	/// Should all features be deleted that have no properties?
	remove_empty_properties: bool,
	/// By default, only the new values without the id are added. Set "add_id" to include the id field.
	add_id: bool,
	child: Box<OperationKDLEnum>,
}

#[derive(Debug)]
pub struct Runner {
	args: Args,
	tile_compression: TileCompression,
	properties_map: HashMap<String, GeoProperties>,
}

impl Runner {
	fn run(&self, mut blob: Blob) -> Result<Option<Blob>> {
		blob = decompress(blob, &self.tile_compression)?;
		let mut tile =
			VectorTile::from_blob(&blob).context("Failed to create VectorTile from Blob")?;

		let layer_name = self.args.layer_name.as_ref();

		for layer in tile.layers.iter_mut() {
			if layer_name.map_or(false, |layer_name| &layer.name != layer_name) {
				continue;
			}

			layer.map_properties(|properties| {
				if let Some(mut prop) = properties {
					if let Some(id) = prop.get(&self.args.id_field_tiles) {
						if let Some(new_prop) = self.properties_map.get(&id.to_string()) {
							if self.args.replace_properties {
								prop = new_prop.clone();
							} else {
								prop.update(new_prop.clone());
							}
							return Some(prop);
						} else {
							warn!("id \"{id}\" not found in data source");
						}
					} else {
						warn!("id field \"{}\" not found", &self.args.id_field_tiles);
					}
				}
				None
			})?;

			if self.args.remove_empty_properties {
				layer.retain_features(|feature| !feature.tag_ids.is_empty());
			}
		}

		Ok(Some(
			tile
				.to_blob()
				.context("Failed to convert VectorTile to Blob")?,
		))
	}
}

#[derive(Debug)]
pub struct Operation {
	runner: Arc<Runner>,
	parameters: TilesReaderParameters,
	source: Box<dyn OperationTrait>,
	meta: Option<Blob>,
}

impl<'a> Operation {
	pub fn new(args: Args, factory: &'a Factory) -> BoxFuture<'a, Result<Self>> {
		Box::pin(async move {
			let data = read_csv_file(&factory.resolve_path(&args.data_source_path))
				.with_context(|| format!("Failed to read CSV file from '{}'", args.data_source_path))?;

			let properties_map = data
				.into_iter()
				.map(|mut properties| {
					let key = properties
						.get(&args.id_field_values)
						.ok_or_else(|| anyhow!("Key '{}' not found in CSV data", args.id_field_values))
						.with_context(|| {
							format!(
								"Failed to find key '{}' in the CSV data row: {properties:?}",
								args.id_field_values
							)
						})?
						.to_string();
					if !args.add_id {
						properties.remove(&args.id_field_values)
					}
					Ok((key, properties))
				})
				.collect::<Result<HashMap<String, GeoProperties>>>()
				.context("Failed to build properties map from CSV data")?;

			let source = factory.build_operation(*args.child.clone()).await?;
			let parameters = source.get_parameters().clone();
			ensure!(
				parameters.tile_format == TileFormat::PBF,
				"source must be vector tiles"
			);

			let meta = source.get_meta();

			let runner = Arc::new(Runner {
				args,
				properties_map,
				tile_compression: parameters.tile_compression,
			});

			Ok(Self {
				runner,
				meta,
				parameters,
				source,
			})
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}
	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		let runner = self.runner.clone();
		self
			.source
			.get_bbox_tile_stream(bbox)
			.await
			.filter_map_blob_parallel(move |blob| runner.run(blob).unwrap())
	}
	fn get_meta(&self) -> Option<Blob> {
		self.meta.clone()
	}
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		Ok(
			if let Some(blob) = self.source.get_tile_data(coord).await? {
				self.runner.run(blob)?
			} else {
				None
			},
		)
	}
}
