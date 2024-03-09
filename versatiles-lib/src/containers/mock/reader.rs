use crate::{
	containers::{TileReaderBox, TileReaderTrait},
	create_error,
	shared::{compress_gzip, Blob, Compression, TileBBoxPyramid, TileCoord3, TileFormat, TileReaderParameters},
};
use anyhow::Result;
use async_trait::async_trait;

#[derive(Debug)]
pub enum ReaderProfile {
	JSON,
	PNG,
	PBF,
}

pub const BYTES_PNG: &[u8; 103] = include_bytes!("./mock.png");
pub const BYTES_PBF: &[u8; 54] = include_bytes!("./mock.pbf");

pub struct TileReader {
	parameters: TileReaderParameters,
	profile: ReaderProfile,
}

impl TileReader {
	pub fn new_mock(profile: ReaderProfile, max_zoom_level: u8) -> TileReaderBox {
		let mut bbox_pyramid = TileBBoxPyramid::new_full();
		bbox_pyramid.set_zoom_max(max_zoom_level);

		let parameters = match profile {
			ReaderProfile::JSON => TileReaderParameters::new(TileFormat::JSON, Compression::None, bbox_pyramid),
			ReaderProfile::PNG => TileReaderParameters::new(TileFormat::PNG, Compression::None, bbox_pyramid),
			ReaderProfile::PBF => TileReaderParameters::new(TileFormat::PBF, Compression::Gzip, bbox_pyramid),
		};

		Box::new(Self { profile, parameters })
	}
}

#[async_trait]
impl TileReaderTrait for TileReader {
	async fn new(_path: &str) -> Result<TileReaderBox> {
		create_error!("don't want to")
	}
	fn get_container_name(&self) -> Result<&str> {
		Ok("dummy container")
	}
	fn get_name(&self) -> Result<&str> {
		Ok("dummy name")
	}
	fn get_parameters(&self) -> Result<&TileReaderParameters> {
		Ok(&self.parameters)
	}
	fn get_parameters_mut(&mut self) -> Result<&mut TileReaderParameters> {
		Ok(&mut self.parameters)
	}
	async fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(Some(Blob::from("dummy meta data")))
	}
	async fn get_tile_data_original(&mut self, coord: &TileCoord3) -> Result<Blob> {
		if coord.is_valid() {
			Ok(match self.profile {
				ReaderProfile::JSON => Blob::from(coord.as_json()),
				ReaderProfile::PNG => Blob::from(BYTES_PNG.to_vec()),
				ReaderProfile::PBF => compress_gzip(Blob::from(BYTES_PBF.to_vec()))?,
			})
		} else {
			create_error!("invalid coordinates: {coord:?}")
		}
	}
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
	use super::{BYTES_PBF, BYTES_PNG};
	use crate::{
		containers::mock::{converter::ConverterProfile, reader::ReaderProfile, TileConverter, TileReader},
		shared::{decompress, Blob, TileCoord3, TileReaderParameters},
	};
	use anyhow::Result;

	#[tokio::test]
	async fn reader() -> Result<()> {
		let mut reader = TileReader::new_mock(ReaderProfile::PNG, 8);
		assert_eq!(reader.get_container_name()?, "dummy container");
		assert_eq!(reader.get_name()?, "dummy name");
		assert_ne!(reader.get_parameters()?, &TileReaderParameters::new_dummy());
		assert_ne!(reader.get_parameters_mut()?, &mut TileReaderParameters::new_dummy());
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
			let mut reader = TileReader::new_mock(profile, 8);
			let tile_compressed = reader.get_tile_data_original(&coord).await.unwrap();
			let tile_uncompressed = decompress(tile_compressed, reader.get_tile_compression().unwrap()).unwrap();
			assert_eq!(tile_uncompressed, blob);
		};

		test(ReaderProfile::PNG, Blob::from(BYTES_PNG.to_vec())).await;
		test(ReaderProfile::PBF, Blob::from(BYTES_PBF.to_vec())).await;
		test(ReaderProfile::JSON, Blob::from("{x:23,y:45,z:6}")).await;
	}

	#[tokio::test]
	async fn convert_from() {
		let mut converter = TileConverter::new_mock(ConverterProfile::PNG, 8);
		let mut reader = TileReader::new_mock(ReaderProfile::PNG, 8);
		converter.convert_from(&mut reader).await.unwrap();
	}
}
