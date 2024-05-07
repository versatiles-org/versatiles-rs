use super::{ProbeDepth, TilesStream};
use crate::shared::{Blob, Compression, PrettyPrint, TileBBox, TileCoord3, TileFormat, TilesReaderParameters};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex;

pub type TilesReaderBox = Box<dyn TilesReaderTrait>;

#[allow(clippy::new_ret_no_self)]
#[async_trait]
pub trait TilesReaderTrait: Debug + Send + Sync + Unpin {
	/// some kine of name for this reader source, e.g. the filename
	fn get_name(&self) -> &str;

	fn get_parameters(&self) -> &TilesReaderParameters;

	fn get_parameters_mut(&mut self) -> &mut TilesReaderParameters;

	fn set_configuration(&mut self, flip_y: bool, swap_xy: bool, tile_compression: Option<Compression>) {
		let parameters = self.get_parameters_mut();
		parameters.swap_xy = swap_xy;
		parameters.flip_y = flip_y;

		if let Some(compression) = tile_compression {
			parameters.tile_compression = compression;
		}
	}

	fn get_tile_format(&self) -> &TileFormat {
		&self.get_parameters().tile_format
	}

	fn get_tile_compression(&self) -> &Compression {
		&self.get_parameters().tile_compression
	}

	/// container name, e.g. versatiles, mbtiles, ...
	fn get_container_name(&self) -> &str;

	/// get meta data, always uncompressed
	async fn get_meta(&self) -> Result<Option<Blob>>;

	/// always compressed with get_tile_compression and formatted with get_tile_format
	/// returns the tile in the coordinate system of the source
	async fn get_tile_data_original(&mut self, coord: &TileCoord3) -> Result<Blob>;

	/// always compressed with get_tile_compression and formatted with get_tile_format
	/// returns the tile in the target coordinate system (after optional flipping)
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Blob> {
		let mut coord_inner: TileCoord3 = *coord;
		self.get_parameters().transform_backward(&mut coord_inner);
		self.get_tile_data_original(&coord_inner).await
	}

	/// always compressed with get_tile_compression and formatted with get_tile_format
	/// returns the tiles in the coordinate system of the source
	async fn get_bbox_tile_stream_original<'a>(&'a mut self, bbox: TileBBox) -> TilesStream {
		let mutex = Arc::new(Mutex::new(self));
		let coords: Vec<TileCoord3> = bbox.iter_coords().collect();
		stream::iter(coords)
			.filter_map(move |coord| {
				let mutex = mutex.clone();
				async move {
					mutex
						.lock()
						.await
						.get_tile_data_original(&coord)
						.await
						.map(|blob| (coord, blob))
						.ok()
				}
			})
			.boxed()
	}

	/// always compressed with get_tile_compression and formatted with get_tile_format
	/// returns the tiles in the target coordinate system (after optional flipping)
	async fn get_bbox_tile_stream<'a>(&'a mut self, mut bbox: TileBBox) -> TilesStream {
		let parameters: TilesReaderParameters = (*self.get_parameters()).clone();
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
	async fn probe(&mut self, level: ProbeDepth) -> Result<()> {
		use ProbeDepth::*;

		let mut print = PrettyPrint::new();

		let cat = print.get_category("meta_data").await;
		cat.add_key_value("name", self.get_name()).await;
		cat.add_key_value("container", self.get_container_name()).await;

		let meta_option = self.get_meta().await?;
		if let Some(meta) = meta_option {
			cat.add_key_value("meta", meta.as_str()).await;
		} else {
			cat.add_key_value("meta", &meta_option).await;
		}

		self
			.get_parameters()
			.probe(print.get_category("parameters").await)
			.await?;

		if matches!(level, Container | Tiles | TileContents) {
			self.probe_container(print.get_category("container").await).await?;
		}

		if matches!(level, Tiles | TileContents) {
			self.probe_tiles(print.get_category("tiles").await).await?;
		}

		if matches!(level, TileContents) {
			self
				.probe_tile_contents(print.get_category("tile contents").await)
				.await?;
		}

		Ok(())
	}

	#[cfg(feature = "full")]
	/// deep probe container
	async fn probe_container(&mut self, print: PrettyPrint) -> Result<()> {
		print
			.add_warning("deep container probing is not implemented for this container format")
			.await;
		Ok(())
	}

	#[cfg(feature = "full")]
	/// deep probe container tiles
	async fn probe_tiles(&mut self, print: PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tiles probing is not implemented for this container format")
			.await;
		Ok(())
	}

	#[cfg(feature = "full")]
	/// deep probe container tile contents
	async fn probe_tile_contents(&mut self, print: PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tile contents probing is not implemented for this container format")
			.await;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[derive(Debug)]
	struct TestReader {
		name: String,
		parameters: TilesReaderParameters,
	}

	impl TestReader {
		async fn new_dummy(path: &str) -> Result<TilesReaderBox> {
			let parameters = TilesReaderParameters::new_dummy();
			let reader = TestReader {
				name: path.to_owned(),
				parameters,
			};
			Ok(Box::new(reader))
		}
	}

	#[async_trait]
	impl TilesReaderTrait for TestReader {
		fn get_name(&self) -> &str {
			&self.name
		}

		fn get_parameters(&self) -> &TilesReaderParameters {
			&self.parameters
		}

		fn get_parameters_mut(&mut self) -> &mut TilesReaderParameters {
			&mut self.parameters
		}

		async fn get_meta(&self) -> Result<Option<Blob>> {
			Ok(Some(Blob::from("test metadata")))
		}

		fn get_container_name(&self) -> &str {
			"test container name"
		}

		async fn get_tile_data_original(&mut self, _coord: &TileCoord3) -> Result<Blob> {
			Ok(Blob::from("test tile data"))
		}
	}

	#[tokio::test]
	#[cfg(feature = "full")]
	async fn reader() -> Result<()> {
		use crate::containers::{MockTilesWriter, MockTilesWriterProfile};

		let mut reader = TestReader::new_dummy("test_path").await?;

		// Test getting name
		assert_eq!(reader.get_name(), "test_path");

		// Test getting tile compression and format
		assert_eq!(reader.get_tile_compression(), &Compression::None);
		assert_eq!(reader.get_tile_format(), &TileFormat::PBF);

		// Test getting container name
		assert_eq!(reader.get_container_name(), "test container name");

		// Test getting metadata
		assert_eq!(reader.get_meta().await?.unwrap().to_string(), "test metadata");

		// Test getting tile data
		let coord = TileCoord3::new(0, 0, 0)?;
		assert_eq!(
			reader.get_tile_data_original(&coord).await?.to_string(),
			"test tile data"
		);

		let mut converter = MockTilesWriter::new_mock(MockTilesWriterProfile::Whatever, 3);
		converter.convert_from(&mut reader).await?;

		Ok(())
	}

	#[tokio::test]
	async fn get_bbox_tile_iter() -> Result<()> {
		let mut reader = TestReader::new_dummy("test_path").await?;
		let bbox = TileBBox::new(4, 0, 0, 10, 10)?; // Or replace it with actual bbox
		let mut stream = reader.get_bbox_tile_stream(bbox).await;

		while let Some((_coord, _blob)) = stream.next().await {}

		Ok(())
	}
}
