mod pbf_update_properties;
mod read;

#[cfg(test)]
mod pbf_mock;

pub use super::lookup::TileComposerOperationLookup;
use crate::{
	container::{TilesReaderParameters, TilesStream},
	utils::YamlWrapper,
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_core::types::{Blob, TileBBox, TileCoord3};

/// The `TileComposerOperation` trait defines the interface for operations that can be applied
/// to tiles in the Composer module.
#[async_trait]
pub trait TileComposerOperation: Debug + Send + Sync {
	/// Creates a new instance of the operation from the provided YAML configuration.
	///
	/// # Arguments
	///
	/// * `def` - A reference to a `YamlWrapper` containing the configuration.
	///
	/// # Returns
	///
	/// * `Result<Self>` - The constructed operation or an error if the configuration is invalid.
	async fn new(
		name: &str,
		yaml: YamlWrapper,
		lookup: &mut TileComposerOperationLookup,
	) -> Result<Self>
	where
		Self: Sized;

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TilesStream;
	async fn get_parameters(&self) -> &TilesReaderParameters;
	async fn get_meta(&self) -> Result<Option<Blob>>;
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>>;
}

/// Creates a new tile composer operation based on the provided YAML configuration.
///
/// # Arguments
///
/// * `def` - A reference to a `YamlWrapper` containing the configuration.
///
/// # Returns
///
/// * `Result<Box<dyn TileComposerOperation>>` - The constructed operation or an error if the configuration is invalid.
pub async fn new_tile_composer_operation(
	name: &str,
	yaml: YamlWrapper,
	lookup: &mut TileComposerOperationLookup,
) -> Result<Box<dyn TileComposerOperation>> {
	let action = yaml
		.hash_get_str("action")
		.context("while parsing action")?
		.to_owned();

	let args = (name, yaml, lookup);

	async fn build<T: TileComposerOperation + 'static>(
		args: (&str, YamlWrapper, &mut TileComposerOperationLookup),
	) -> Result<Box<dyn TileComposerOperation>> {
		T::new(args.0, args.1, args.2)
			.await
			.map(|op| Box::new(op) as Box<dyn TileComposerOperation>)
	}

	let result = match action.as_str() {
		"pbf_update_properties" => build::<pbf_update_properties::Operation>(args).await,
		"read" => build::<read::Operation>(args).await,
		#[cfg(test)]
		"pbf_mock" => build::<pbf_mock::Operation>(args).await,
		_ => Err(anyhow!("operation '{action}' is unknown")),
	};

	result.with_context(|| format!("Failed parsing action '{action}'"))
}
