use crate::{TileReaderBox, TileReaderTrait};
use async_trait::async_trait;
use versatiles_shared::{Blob, Result, TileCoord3, TileReaderParameters};

pub struct TileReader {
	parameters: TileReaderParameters,
}

#[async_trait]
impl TileReaderTrait for TileReader {
	async fn new(_path: &str) -> Result<TileReaderBox> {
		Ok(Box::new(Self {
			parameters: TileReaderParameters::new_dummy(),
		}))
	}
	fn get_container_name(&self) -> &str {
		"dummy"
	}
	fn get_name(&self) -> &str {
		"dummy"
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		&self.parameters
	}
	fn get_parameters_mut(&mut self) -> &mut TileReaderParameters {
		&mut self.parameters
	}
	async fn get_meta(&self) -> Blob {
		Blob::empty()
	}
	async fn get_tile_data(&self, _coord: &TileCoord3) -> Option<Blob> {
		Some(Blob::empty())
	}
	async fn deep_verify(&self) {}
}

impl std::fmt::Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileReader:MBTiles")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}
