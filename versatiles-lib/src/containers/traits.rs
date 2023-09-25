use crate::shared::*;
use async_trait::async_trait;
use futures_util::{stream, Stream, StreamExt};
use std::{fmt::Debug, pin::Pin, sync::Arc};
use tokio::sync::Mutex;

#[cfg(feature = "full")]
pub type TileConverterBox = Box<dyn TileConverterTrait>;
pub type TileReaderBox = Box<dyn TileReaderTrait>;
pub type TileStream<'a> = Pin<Box<dyn Stream<Item = (TileCoord3, Blob)> + Send + 'a>>;

#[allow(clippy::new_ret_no_self)]
#[async_trait]
#[cfg(feature = "full")]
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
		Ok(&self.get_parameters()?.tile_format)
	}

	fn get_tile_compression(&self) -> Result<&Compression> {
		Ok(&self.get_parameters()?.tile_compression)
	}

	/// container name, e.g. versatiles, mbtiles, ...
	fn get_container_name(&self) -> Result<&str>;

	/// get meta data, always uncompressed
	async fn get_meta(&self) -> Result<Option<Blob>>;

	/// always compressed with get_tile_compression and formatted with get_tile_format
	/// returns the tile in the coordinate system of the source
	async fn get_tile_data_original(&mut self, coord: &TileCoord3) -> Result<Blob>;

	/// always compressed with get_tile_compression and formatted with get_tile_format
	/// returns the tile in the target coordinate system (after optional flipping)
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Blob> {
		let mut coord_inner: TileCoord3 = *coord;
		self.get_parameters()?.transform_backward(&mut coord_inner);
		self.get_tile_data_original(&coord_inner).await
	}

	/// always compressed with get_tile_compression and formatted with get_tile_format
	/// returns the tiles in the coordinate system of the source
	async fn get_bbox_tile_stream_original<'a>(&'a mut self, bbox: TileBBox) -> TileStream {
		let mutex = Arc::new(Mutex::new(self));
		let coords: Vec<TileCoord3> = bbox.iter_coords().collect();
		stream::iter(coords)
			.filter_map(move |coord| {
				let mutex = mutex.clone();
				async move {
					let result = mutex.lock().await.get_tile_data_original(&coord).await;
					if result.is_err() {
						return None;
					}
					let blob = result.unwrap();
					Some((coord, blob))
				}
			})
			.boxed()
	}

	/// always compressed with get_tile_compression and formatted with get_tile_format
	/// returns the tiles in the target coordinate system (after optional flipping)
	async fn get_bbox_tile_stream<'a>(&'a mut self, mut bbox: TileBBox) -> TileStream {
		let parameters: TileReaderParameters = (*self.get_parameters().unwrap()).clone();
		parameters.transform_backward(&mut bbox);
		let stream = self.get_bbox_tile_stream_original(bbox).await;
		stream
			.map(move |(mut coord, blob)| {
				parameters.transform_forward(&mut coord);
				(coord, blob)
			})
			.boxed()
	}

	#[cfg(feature = "full")]
	/// probe container
	async fn probe(&mut self, level: u8) -> Result<()> {
		let mut print = PrettyPrint::new();

		let cat = print.get_category("parameters").await;
		cat.add_key_value("name", self.get_name()?).await;
		cat.add_key_value("container", self.get_container_name()?).await;
		let meta_option = self.get_meta().await?;
		if let Some(meta) = meta_option {
			cat.add_key_value("meta", meta.as_str()).await;
		} else {
			cat.add_key_value("meta", &meta_option).await;
		}

		self
			.get_parameters()?
			.probe(print.get_category("parameters").await)
			.await?;

		if level >= 1 {
			self.probe_container(print.get_category("container").await).await?;
		}

		if level >= 2 {
			self.probe_tiles(print.get_category("container_tiles").await).await?;
		}

		Ok(())
	}

	#[cfg(feature = "full")]
	/// probe container deep
	async fn probe_container(&mut self, print: PrettyPrint) -> Result<()> {
		print
			.add_warning("deep container probing is not implemented for this container format")
			.await;
		Ok(())
	}

	#[cfg(feature = "full")]
	/// probe container
	async fn probe_tiles(&mut self, print: PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tile probing is not implemented for this container format")
			.await;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use async_trait::async_trait;
	use futures_util::StreamExt;

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

		async fn get_meta(&self) -> Result<Option<Blob>> {
			Ok(Some(Blob::from("test metadata")))
		}

		fn get_container_name(&self) -> Result<&str> {
			Ok("test container name")
		}

		async fn get_tile_data_original(&mut self, _coord: &TileCoord3) -> Result<Blob> {
			Ok(Blob::from("test tile data"))
		}
	}
	#[cfg(feature = "full")]
	struct TestConverter {}

	#[async_trait]
	#[cfg(feature = "full")]
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
	#[cfg(feature = "full")]
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
		assert_eq!(reader.get_meta().await?.unwrap().to_string(), "test metadata");

		// Test getting tile data
		let coord = TileCoord3::new(0, 0, 0);
		assert_eq!(
			reader.get_tile_data_original(&coord).await.unwrap().to_string(),
			"test tile data"
		);

		let mut converter = TestConverter::new("/hello", TileConverterConfig::new_full()).await?;
		converter.convert_from(&mut reader).await?;

		Ok(())
	}

	#[tokio::test]
	async fn get_bbox_tile_iter() -> Result<()> {
		let mut reader = TestReader::new("test_path").await?;
		let bbox = TileBBox::new(4, 0, 0, 10, 10); // Or replace it with actual bbox
		let mut stream = reader.get_bbox_tile_stream(bbox).await;

		while let Some((_coord, _blob)) = stream.next().await {}

		Ok(())
	}
}
