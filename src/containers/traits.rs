use crate::shared::*;
use async_trait::async_trait;
use futures::executor::block_on;
use std::{fmt::Debug, path::Path};

pub type TileConverterBox = Box<dyn TileConverterTrait>;
pub type TileReaderBox = Box<dyn TileReaderTrait>;
pub type TileIterator<'a> = Box<dyn Iterator<Item = (TileCoord3, Blob)> + 'a>;

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
pub trait TileReaderTrait: Debug + Send + Sync + Unpin {
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
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Blob>;

	/// always compressed with get_tile_compression and formatted with get_tile_format
	fn get_bbox_tile_iter<'a>(&'a mut self, bbox: &'a TileBBox) -> TileIterator {
		Box::new(bbox.iter_coords().filter_map(|coord| {
			let result = block_on(self.get_tile_data(&coord));
			match result {
				Ok(blob) => Some((coord, blob)),
				Err(_) => None,
			}
		}))
	}

	/// verify container and output data to output_folder
	async fn deep_verify(&mut self, _output_folder: &Path) -> Result<()> {
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

		async fn get_tile_data(&mut self, _coord: &TileCoord3) -> Result<Blob> {
			Ok(Blob::from("test tile data"))
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
	async fn reader() -> Result<()> {
		let mut reader = TestReader::new("test_path").await?;

		// Test getting name
		assert_eq!(reader.get_name()?, "test_path");

		// Test getting tile compression and format
		assert_eq!(reader.get_tile_compression()?, &Compression::None);
		assert_eq!(reader.get_tile_format()?, &TileFormat::PBF);

		// Test getting container name
		assert_eq!(reader.get_container_name()?, "test container name");

		// Test getting metadata
		assert_eq!(reader.get_meta().await?.to_string(), "test metadata");

		// Test getting tile data
		let coord = TileCoord3::new(0, 0, 0);
		assert_eq!(
			reader.get_tile_data(&coord).await.unwrap().to_string(),
			"test tile data"
		);

		let mut converter = TestConverter::new("/hallo", TileConverterConfig::new_full()).await?;
		converter.convert_from(&mut reader).await?;

		Ok(())
	}

	#[tokio::test]
	async fn get_bbox_tile_stream() -> Result<()> {
		let mut reader = TestReader::new("test_path").await?;
		let bbox = TileBBox::new(4, 0, 0, 10, 10); // Or replace it with actual bbox
		let vec = reader.get_bbox_tile_iter(&bbox);

		for (coord, blob) in vec {
			println!("TileCoord2: {:?}", coord);
			println!("Blob: {:?}", blob);
			// Here, you can add the assertions you need to verify the correctness of each tile data
		}

		Ok(())
	}
}
