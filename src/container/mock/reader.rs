use crate::{
	container::{TilesReaderBox, TilesReaderParameters, TilesReaderTrait},
	helper::compress,
	types::{Blob, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat},
};
use anyhow::Result;

#[derive(Debug)]
pub enum MockTilesReaderProfile {
	JSON,
	PNG,
	PBF,
}

//pub const MOCK_BYTES_AVIF: &[u8; 323] = include_bytes!("./mock_tiles/mock.avif");
pub const MOCK_BYTES_JPG: &[u8; 671] = include_bytes!("./mock_tiles/mock.jpg");
pub const MOCK_BYTES_PBF: &[u8; 54] = include_bytes!("./mock_tiles/mock.pbf");
pub const MOCK_BYTES_PNG: &[u8; 103] = include_bytes!("./mock_tiles/mock.png");
pub const MOCK_BYTES_WEBP: &[u8; 44] = include_bytes!("./mock_tiles/mock.webp");

pub struct MockTilesReader {
	parameters: TilesReaderParameters,
}

impl MockTilesReader {
	pub fn new_mock_profile(profile: MockTilesReaderProfile) -> TilesReaderBox {
		let bbox_pyramid = TileBBoxPyramid::new_full(4);

		Self::new_mock(match profile {
			MockTilesReaderProfile::JSON => {
				TilesReaderParameters::new(TileFormat::JSON, TileCompression::None, bbox_pyramid)
			}
			MockTilesReaderProfile::PNG => {
				TilesReaderParameters::new(TileFormat::PNG, TileCompression::None, bbox_pyramid)
			}
			MockTilesReaderProfile::PBF => {
				TilesReaderParameters::new(TileFormat::PBF, TileCompression::Gzip, bbox_pyramid)
			}
		})
	}
	pub fn new_mock(parameters: TilesReaderParameters) -> TilesReaderBox {
		Box::new(Self { parameters })
	}
}

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
	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.parameters.tile_compression = tile_compression;
	}
	fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(Some(Blob::from("dummy meta data")))
	}
	fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		if !coord.is_valid() {
			return Ok(None);
		}

		let format = self.parameters.tile_format;
		let mut blob = match format {
			TileFormat::JSON => Blob::from(coord.as_json()),
			TileFormat::PNG => Blob::from(MOCK_BYTES_PNG.to_vec()),
			TileFormat::PBF => Blob::from(MOCK_BYTES_PBF.to_vec()),
			//TileFormat::AVIF => Blob::from(MOCK_BYTES_AVIF.to_vec()),
			TileFormat::JPG => Blob::from(MOCK_BYTES_JPG.to_vec()),
			TileFormat::WEBP => Blob::from(MOCK_BYTES_WEBP.to_vec()),
			_ => panic!("tile format {format:?} is not implemented for MockTileReader"),
		};
		blob = compress(blob, &self.parameters.tile_compression)?;
		Ok(Some(blob))
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
	use super::*;
	use crate::{container::mock::MockTilesWriter, helper::decompress};
	use anyhow::Result;

	#[test]
	fn reader() -> Result<()> {
		let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::PNG);
		assert_eq!(reader.get_container_name(), "dummy_container");
		assert_eq!(reader.get_name(), "dummy_name");

		let bbox_pyramid = TileBBoxPyramid::new_full(4);

		assert_eq!(
			reader.get_parameters(),
			&TilesReaderParameters::new(TileFormat::PNG, TileCompression::None, bbox_pyramid)
		);
		assert_eq!(reader.get_meta()?, Some(Blob::from("dummy meta data")));
		let blob = reader.get_tile_data(&TileCoord3::new(0, 0, 0)?)?.unwrap().as_vec();
		assert_eq!(&blob[0..4], b"\x89PNG");
		Ok(())
	}

	#[test]
	fn get_tile_data() {
		let test = move |profile, blob| {
			let coord = TileCoord3::new(23, 45, 6).unwrap();
			let mut reader = MockTilesReader::new_mock_profile(profile);
			let tile_compressed = reader.get_tile_data(&coord).unwrap().unwrap();
			let tile_uncompressed = decompress(tile_compressed, &reader.get_parameters().tile_compression).unwrap();
			assert_eq!(tile_uncompressed, blob);
		};

		test(MockTilesReaderProfile::PNG, Blob::from(MOCK_BYTES_PNG.to_vec()));
		test(MockTilesReaderProfile::PBF, Blob::from(MOCK_BYTES_PBF.to_vec()));
		test(MockTilesReaderProfile::JSON, Blob::from("{x:23,y:45,z:6}"));
	}

	#[tokio::test]
	async fn convert_from() {
		let mut writer = MockTilesWriter::new_mock();
		let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::PNG);
		writer.write_from_reader(&mut reader).await.unwrap();
	}
}
