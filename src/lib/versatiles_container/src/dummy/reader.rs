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
		"dummy container"
	}
	fn get_name(&self) -> &str {
		"dummy name"
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		&self.parameters
	}
	fn get_parameters_mut(&mut self) -> &mut TileReaderParameters {
		&mut self.parameters
	}
	async fn get_meta(&self) -> Blob {
		Blob::from("dummy meta data")
	}
	async fn get_tile_data(&self, _coord: &TileCoord3) -> Option<Blob> {
		Some(Blob::from("dummy tile data"))
	}
	async fn deep_verify(&self) {}
}

impl std::fmt::Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileReader:Dummy")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use crate::{dummy, TileConverterTrait, TileReaderTrait};
	use futures::executor::block_on;
	use versatiles_shared::{Blob, TileCoord3, TileReaderParameters};

	#[test]
	fn test1() {
		let mut reader = block_on(dummy::TileReader::new("filename.txt")).unwrap();
		assert_eq!(reader.get_container_name(), "dummy container");
		assert_eq!(reader.get_name(), "dummy name");
		assert_eq!(reader.get_parameters(), &TileReaderParameters::new_dummy());
		assert_eq!(reader.get_parameters_mut(), &mut TileReaderParameters::new_dummy());
		assert_eq!(block_on(reader.get_meta()), Blob::from("dummy meta data"));
		assert_eq!(
			block_on(reader.get_tile_data(&TileCoord3::new_empty())).unwrap(),
			Blob::from("dummy tile data")
		);
	}

	#[test]
	fn test2() {
		let mut converter = dummy::TileConverter {};
		let mut reader = block_on(dummy::TileReader::new("filename.txt")).unwrap();
		block_on(converter.convert_from(&mut reader));
	}
}
