use crate::{
	containers::{TilesReaderBox, TilesReaderTrait},
	shared::{compress_gzip, Blob, Compression, TileBBoxPyramid, TileCoord3, TileFormat, TilesReaderParameters},
};
use anyhow::{bail, Result};
use async_trait::async_trait;

#[derive(Debug)]
pub enum MockTilesReaderProfile {
	JSON,
	PNG,
	PBF,
}

pub const MOCK_BYTES_PNG: &[u8; 103] = include_bytes!("./mock.png");
pub const MOCK_BYTES_PBF: &[u8; 54] = include_bytes!("./mock.pbf");

pub struct MockTilesReader {
	parameters: TilesReaderParameters,
	profile: MockTilesReaderProfile,
}

impl MockTilesReader {
	pub fn new_mock(profile: MockTilesReaderProfile, max_zoom_level: u8) -> TilesReaderBox {
		let mut bbox_pyramid = TileBBoxPyramid::new_full();
		bbox_pyramid.set_zoom_max(max_zoom_level);

		let parameters = match profile {
			MockTilesReaderProfile::JSON => TilesReaderParameters::new(TileFormat::JSON, Compression::None, bbox_pyramid),
			MockTilesReaderProfile::PNG => TilesReaderParameters::new(TileFormat::PNG, Compression::None, bbox_pyramid),
			MockTilesReaderProfile::PBF => TilesReaderParameters::new(TileFormat::PBF, Compression::Gzip, bbox_pyramid),
		};

		Box::new(Self { profile, parameters })
	}
}

#[async_trait]
impl TilesReaderTrait for MockTilesReader {
	fn get_container_name(&self) -> &str {
		"dummy_container"
	}
	fn get_name(&self) -> &str {
		"dummy_name"
	}
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}
	fn get_parameters_mut(&mut self) -> &mut TilesReaderParameters {
		&mut self.parameters
	}
	async fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(Some(Blob::from("dummy meta data")))
	}
	async fn get_tile_data_original(&mut self, coord: &TileCoord3) -> Result<Blob> {
		if coord.is_valid() {
			Ok(match self.profile {
				MockTilesReaderProfile::JSON => Blob::from(coord.as_json()),
				MockTilesReaderProfile::PNG => Blob::from(MOCK_BYTES_PNG.to_vec()),
				MockTilesReaderProfile::PBF => compress_gzip(Blob::from(MOCK_BYTES_PBF.to_vec()))?,
			})
		} else {
			bail!("invalid coordinates: {coord:?}")
		}
	}
}

impl std::fmt::Debug for MockTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MockTilesReader")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::{MOCK_BYTES_PBF, MOCK_BYTES_PNG};
	use crate::{
		containers::mock::{
			reader::MockTilesReaderProfile, writer::MockTilesWriterProfile, MockTilesReader, MockTilesWriter,
		},
		shared::{decompress, Blob, TileCoord3, TilesReaderParameters},
	};
	use anyhow::Result;

	#[tokio::test]
	async fn reader() -> Result<()> {
		let mut reader = MockTilesReader::new_mock(MockTilesReaderProfile::PNG, 8);
		assert_eq!(reader.get_container_name(), "dummy_container");
		assert_eq!(reader.get_name(), "dummy_name");
		assert_ne!(reader.get_parameters(), &TilesReaderParameters::new_dummy());
		assert_ne!(reader.get_parameters_mut(), &mut TilesReaderParameters::new_dummy());
		assert_eq!(reader.get_meta().await?, Some(Blob::from("dummy meta data")));
		let blob = reader
			.get_tile_data_original(&TileCoord3::new(0, 0, 0)?)
			.await?
			.as_vec();
		assert_eq!(&blob[0..4], b"\x89PNG");
		Ok(())
	}

	#[tokio::test]
	async fn get_tile_data_original() {
		let test = |profile, blob| async move {
			let coord = TileCoord3::new(23, 45, 6).unwrap();
			let mut reader = MockTilesReader::new_mock(profile, 8);
			let tile_compressed = reader.get_tile_data_original(&coord).await.unwrap();
			let tile_uncompressed = decompress(tile_compressed, reader.get_tile_compression()).unwrap();
			assert_eq!(tile_uncompressed, blob);
		};

		test(MockTilesReaderProfile::PNG, Blob::from(MOCK_BYTES_PNG.to_vec())).await;
		test(MockTilesReaderProfile::PBF, Blob::from(MOCK_BYTES_PBF.to_vec())).await;
		test(MockTilesReaderProfile::JSON, Blob::from("{x:23,y:45,z:6}")).await;
	}

	#[tokio::test]
	async fn convert_from() {
		let mut converter = MockTilesWriter::new_mock(MockTilesWriterProfile::PNG, 8);
		let mut reader = MockTilesReader::new_mock(MockTilesReaderProfile::PNG, 8);
		converter.convert_from(&mut reader).await.unwrap();
	}
}
