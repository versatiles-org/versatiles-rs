use crate::{napi_result, runtime::create_runtime, types::ReaderParameters};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::sync::Arc;
use tokio::sync::Mutex;
use versatiles_container::TilesReaderTrait;
use versatiles_core::TileCoord as RustTileCoord;

/// Container reader for accessing tile data from various formats
#[napi]
pub struct ContainerReader {
	reader: Arc<Mutex<Box<dyn TilesReaderTrait>>>,
}

#[napi]
impl ContainerReader {
	/// Open a tile container from a file path or URL
	///
	/// File support: .versatiles, .mbtiles, .pmtiles, .tar, directories
	/// URL support: .versatiles, .pmtiles
	#[napi(factory)]
	pub async fn open(path: String) -> Result<Self> {
		let runtime = create_runtime();
		let reader = napi_result!(runtime.get_reader_from_str(&path).await)?;

		Ok(Self {
			reader: Arc::new(Mutex::new(reader)),
		})
	}

	/// Get a single tile at the specified coordinates
	///
	/// Returns null if the tile doesn't exist
	#[napi]
	pub async fn get_tile(&self, z: u32, x: u32, y: u32) -> Result<Option<Buffer>> {
		let coord = napi_result!(RustTileCoord::new(z as u8, x, y))?;
		let reader = self.reader.lock().await;
		let tile_opt = napi_result!(reader.get_tile(&coord).await)?;

		Ok(tile_opt.map(|mut tile| {
			let blob = tile.as_blob(versatiles_core::TileCompression::Uncompressed).unwrap();
			Buffer::from(blob.as_slice())
		}))
	}

	/// Get TileJSON metadata as a JSON string
	#[napi(getter)]
	pub async fn tile_json(&self) -> String {
		let reader = self.reader.lock().await;
		reader.tilejson().as_string()
	}

	/// Get reader parameters (format, compression, zoom levels)
	#[napi(getter)]
	pub async fn parameters(&self) -> ReaderParameters {
		let reader = self.reader.lock().await;
		ReaderParameters::from(reader.parameters())
	}

	/// Get the source name
	#[napi(getter)]
	pub async fn source_name(&self) -> String {
		self.reader.lock().await.source_name().to_string()
	}

	/// Get the container type name
	#[napi(getter)]
	pub async fn container_name(&self) -> String {
		self.reader.lock().await.container_name().to_string()
	}
}
