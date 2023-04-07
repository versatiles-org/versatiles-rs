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
	fn new(filename: &Path, config: TileConverterConfig) -> TileConverterBox
	where
		Self: Sized;

	// readers must be mutable, because they might use caching
	async fn convert_from(&mut self, reader: &mut TileReaderBox);
}

#[allow(clippy::new_ret_no_self)]
#[async_trait]
pub trait TileReaderTrait: Debug + Send + Sync {
	async fn new(path: &str) -> Result<TileReaderBox>
	where
		Self: Sized;

	/// some kine of name for this reader source, e.g. the filename
	fn get_name(&self) -> &str;

	fn get_parameters(&self) -> &TileReaderParameters;

	fn get_parameters_mut(&mut self) -> &mut TileReaderParameters;

	fn get_tile_format(&self) -> &TileFormat {
		self.get_parameters().get_tile_format()
	}

	fn get_tile_compression(&self) -> &Compression {
		self.get_parameters().get_tile_compression()
	}

	/// container name, e.g. versatiles, mbtiles, ...
	fn get_container_name(&self) -> &str;

	/// get meta data, always uncompressed
	async fn get_meta(&self) -> Blob;

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
	async fn deep_verify(&self, _output_folder: &Path) {
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

		fn get_name(&self) -> &str {
			&self.name
		}

		fn get_parameters(&self) -> &TileReaderParameters {
			&self.parameters
		}

		fn get_parameters_mut(&mut self) -> &mut TileReaderParameters {
			&mut self.parameters
		}

		async fn get_meta(&self) -> Blob {
			Blob::from("test metadata")
		}

		fn get_container_name(&self) -> &str {
			"test container name"
		}

		async fn get_tile_data(&self, _coord: &TileCoord3) -> Option<Blob> {
			Some(Blob::from("test tile data"))
		}
	}

	struct TestConverter {}

	#[async_trait]
	impl TileConverterTrait for TestConverter {
		fn new(_filename: &Path, _config: TileConverterConfig) -> TileConverterBox
		where
			Self: Sized,
		{
			let converter = TestConverter {};
			Box::new(converter)
		}

		async fn convert_from(&mut self, _reader: &mut TileReaderBox) {}
	}

	#[tokio::test]
	async fn test_reader() {
		let mut reader = TestReader::new("test_path").await.unwrap();

		// Test getting name
		assert_eq!(reader.get_name(), "test_path");

		// Test getting tile compression and format
		assert_eq!(reader.get_tile_compression(), &Compression::None);
		assert_eq!(reader.get_tile_format(), &TileFormat::PBF);

		// Test getting container name
		assert_eq!(reader.get_container_name(), "test container name");

		// Test getting metadata
		assert_eq!(reader.get_meta().await.to_string(), "test metadata");

		// Test getting tile data
		let coord = TileCoord3::new(0, 0, 0);
		assert_eq!(
			reader.get_tile_data(&coord).await.unwrap().to_string(),
			"test tile data"
		);

		let mut converter = TestConverter::new(&Path::new("/hallo"), TileConverterConfig::new_full());
		converter.convert_from(&mut reader).await;
	}
}
