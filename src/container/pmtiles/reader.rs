use crate::{
	container::{TilesReaderBox, TilesReaderParameters, TilesReaderTrait},
	types::{Blob, TileBBoxPyramid, TileCompression, TileFormat},
};
use anyhow::{bail, ensure, Result};
use axum::async_trait;
use std::{fmt::Debug, fs, path::Path};

pub struct PMTilesReader {
	parameters: TilesReaderParameters,
}

impl PMTilesReader {
	pub async fn open(path: &Path) -> Result<TilesReaderBox>
	where
		Self: Sized,
	{
		log::trace!("read {path:?}");

		ensure!(path.is_absolute(), "path {path:?} must be absolute");
		ensure!(path.exists(), "path {path:?} does not exist");
		ensure!(path.is_file(), "path {path:?} is not a file");

		Ok(Box::new(PMTilesReader {
			parameters: TilesReaderParameters::new(
				tile_format.expect("tile format must be specified"),
				tile_compression.expect("tile compression must be specified"),
				bbox_pyramid,
			),
		}))
	}

	fn read(path: &Path) -> Result<Blob> {
		Ok(Blob::from(fs::read(path)?))
	}
}

#[async_trait]
impl TilesReaderTrait for PMTilesReader {
	fn get_container_name(&self) -> &str {
		"pmtiles"
	}
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}
	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.parameters.tile_compression = tile_compression;
	}
	async fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(self.meta.clone())
	}
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Blob> {
		log::trace!("get_tile_data_original {:?}", coord);

		if let Some(path) = self.tile_map.get(coord) {
			Self::read(path)
		} else {
			bail!("tile {:?} not found", coord);
		}
	}
	fn get_name(&self) -> &str {
		self.dir.to_str().unwrap()
	}
}

impl Debug for PMTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PMTilesReader")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}
