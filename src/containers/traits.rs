use crate::shared::{
	Blob, Compression, Result, TileBBox, TileConverterConfig, TileCoord2, TileCoord3, TileFormat, TileReaderParameters,
};
use async_trait::async_trait;
use std::{fmt::Debug, path::Path};

pub type TileConverterBox = Box<dyn TileConverterTrait>;
pub type TileReaderBox = Box<dyn TileReaderTrait>;

#[allow(clippy::new_ret_no_self)]
#[async_trait]
pub trait TileConverterTrait {
	async fn new(filename: &str, tile_config: TileConverterConfig) -> Result<TileConverterBox>
	where
		Self: Sized;

	// readers must be mutable, because they might use caching
	async fn convert_from(&mut self, reader: &mut TileReaderBox) -> Result<()>;
}

#[allow(clippy::new_ret_no_self)]
#[async_trait]
pub trait TileReaderTrait: Debug + Send + Sync {
	async fn new(path: &str) -> Result<TileReaderBox>
	where
		Self: Sized;

	/// some kine of name for this reader source, e.g. the filename
	fn get_name(&self) -> Result<&str>;

	fn get_parameters(&self) -> Result<&TileReaderParameters>;

	fn get_parameters_mut(&mut self) -> Result<&mut TileReaderParameters>;

	fn get_tile_format(&self) -> Result<&TileFormat> {
		Ok(self.get_parameters()?.get_tile_format())
	}

	fn get_tile_compression(&self) -> Result<&Compression> {
		Ok(self.get_parameters()?.get_tile_compression())
	}

	/// container name, e.g. versatiles, mbtiles, ...
	fn get_container_name(&self) -> Result<&str>;

	/// get meta data, always uncompressed
	async fn get_meta(&self) -> Result<Blob>;

	/// always compressed with get_tile_compression and formatted with get_tile_format
	async fn get_tile_data(&self, coord: &TileCoord3) -> Option<Blob>;

	/// always compressed with get_tile_compression and formatted with get_tile_format
	async fn get_bbox_tile_vec(&self, zoom: u8, bbox: &TileBBox) -> Vec<(TileCoord2, Blob)> {
		let mut vec: Vec<(TileCoord2, Blob)> = Vec::new();
		for coord in bbox.iter_coords() {
			let option = self.get_tile_data(&coord.with_zoom(zoom)).await;
			if let Some(blob) = option {
				vec.push((coord, blob));
			}
		}
		return vec;
	}

	/// verify container and output data to output_folder
	async fn deep_verify(&self, _output_folder: &Path) -> Result<()> {
		todo!()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use async_trait::async_trait;

	#[derive(Debug)]
	struct TestReader {
		name: String,
		parameters: TileReaderParameters,
	}

	#[async_trait]
	impl TileReaderTrait for TestReader {
		async fn new(path: &str) -> Result<TileReaderBox> {
			let parameters = TileReaderParameters::new_dummy();
			let reader = TestReader {
				name: path.to_owned(),
				parameters,
			};
			Ok(Box::new(reader))
		}

		fn get_name(&self) -> Result<&str> {
			Ok(&self.name)
		}

		fn get_parameters(&self) -> Result<&TileReaderParameters> {
			Ok(&self.parameters)
		}

		fn get_parameters_mut(&mut self) -> Result<&mut TileReaderParameters> {
			Ok(&mut self.parameters)
		}

		async fn get_meta(&self) -> Result<Blob> {
			Ok(Blob::from("test metadata"))
		}

		fn get_container_name(&self) -> Result<&str> {
			Ok("test container name")
		}

		async fn get_tile_data(&self, _coord: &TileCoord3) -> Option<Blob> {
			Some(Blob::from("test tile data"))
		}
	}

	struct TestConverter {}

	#[async_trait]
	impl TileConverterTrait for TestConverter {
		async fn new(_filename: &str, _config: TileConverterConfig) -> Result<TileConverterBox>
		where
			Self: Sized,
		{
			let converter = TestConverter {};
			Ok(Box::new(converter))
		}

		async fn convert_from(&mut self, _reader: &mut TileReaderBox) -> Result<()> {
			Ok(())
		}
	}

	#[tokio::test]
	async fn test_reader() {
		let mut reader = TestReader::new("test_path").await.unwrap();

		// Test getting name
		assert_eq!(reader.get_name().unwrap(), "test_path");

		// Test getting tile compression and format
		assert_eq!(reader.get_tile_compression().unwrap(), &Compression::None);
		assert_eq!(reader.get_tile_format().unwrap(), &TileFormat::PBF);

		// Test getting container name
		assert_eq!(reader.get_container_name().unwrap(), "test container name");

		// Test getting metadata
		assert_eq!(reader.get_meta().await.unwrap().to_string(), "test metadata");

		// Test getting tile data
		let coord = TileCoord3::new(0, 0, 0);
		assert_eq!(
			reader.get_tile_data(&coord).await.unwrap().to_string(),
			"test tile data"
		);

		let mut converter = TestConverter::new("/hallo", TileConverterConfig::new_full())
			.await
			.unwrap();
		converter.convert_from(&mut reader).await.unwrap();
	}
}
