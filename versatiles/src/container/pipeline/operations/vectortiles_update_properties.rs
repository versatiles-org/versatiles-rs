use crate::{
	container::{
		pipeline::utils::{
			read_csv_file, OperationTrait, PipelineFactory, TransformOperationFactoryTrait,
		},
		utils::OperationFactoryTrait,
		TilesReaderParameters,
	},
	geometry::{vector_tile::VectorTile, GeoProperties},
	types::Blob,
	utils::{decompress, vpl::VPLNode},
};
use anyhow::{anyhow, ensure, Context, Result};
use async_trait::async_trait;
use futures::future::BoxFuture;
use log::warn;
use std::{collections::HashMap, sync::Arc};
use versatiles_core::types::{TileBBox, TileCompression, TileCoord3, TileFormat, TileStream};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Updates properties of vector tile features using data from an external source (e.g., CSV file). Matches features based on an ID field.
struct Args {
	/// Path to the data source file, e.g., `data.csv`.
	data_source_path: String,
	/// ID field name in the vector tiles.
	id_field_tiles: String,
	/// ID field name in the data source.
	id_field_data: String,
	/// Name of the layer to update. If unspecified, all layers will be updated.
	layer_name: Option<String>,
	/// If set, old properties will be deleted before new ones are added.
	replace_properties: bool,
	/// If set, features with no properties will be removed.
	remove_empty_properties: bool,
	/// If set, includes the ID field in the updated properties.
	include_id: bool,
}

#[derive(Debug)]
struct Runner {
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
struct Operation {
	runner: Arc<Runner>,
	parameters: TilesReaderParameters,
	source: Box<dyn OperationTrait>,
	meta: Option<Blob>,
}

impl<'a> Operation {
	fn build(
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		factory: &'a PipelineFactory,
	) -> BoxFuture<'a, Result<Box<dyn OperationTrait>, anyhow::Error>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			let data = read_csv_file(&factory.resolve_path(&args.data_source_path))
				.with_context(|| format!("Failed to read CSV file from '{}'", args.data_source_path))?;

			let properties_map = data
				.into_iter()
				.map(|mut properties| {
					let key = properties
						.get(&args.id_field_data)
						.ok_or_else(|| anyhow!("Key '{}' not found in CSV data", args.id_field_data))
						.with_context(|| {
							format!(
								"Failed to find key '{}' in the CSV data row: {properties:?}",
								args.id_field_data
							)
						})?
						.to_string();
					if !args.include_id {
						properties.remove(&args.id_field_data)
					}
					Ok((key, properties))
				})
				.collect::<Result<HashMap<String, GeoProperties>>>()
				.context("Failed to build properties map from CSV data")?;

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

			Ok(Box::new(Self {
				runner,
				meta,
				parameters,
				source,
			}) as Box<dyn OperationTrait>)
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

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"vectortiles_update_properties"
	}
}

#[async_trait]
impl TransformOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>> {
		Operation::build(vpl_node, source, factory).await
	}
}
