use crate::{
	containers::{TileReaderBox, TileReaderTrait},
	create_error,
	shared::{compress_gzip, Blob, Compression, Result, TileBBoxPyramid, TileCoord3, TileFormat, TileReaderParameters},
};
use async_trait::async_trait;

#[derive(Debug)]
pub enum ReaderProfile {
	JSON,
	PNG,
	PBF,
}

pub struct TileReader {
	parameters: TileReaderParameters,
	profile: ReaderProfile,
}

impl TileReader {
	pub fn new_dummy(profile: ReaderProfile, max_zoom_level: u8) -> TileReaderBox {
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
				ReaderProfile::PNG => Blob::from(include_bytes!("./dummy.png").to_vec()),
				ReaderProfile::PBF => compress_gzip(Blob::from(include_bytes!("./dummy.pbf").to_vec())).unwrap(),
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
	use crate::{
		containers::dummy::{converter::ConverterProfile, reader::ReaderProfile, TileConverter, TileReader},
		shared::{Blob, Result, TileCoord3, TileReaderParameters},
	};

	#[tokio::test]
	async fn reader() -> Result<()> {
		let mut reader = TileReader::new_dummy(ReaderProfile::PNG, 8);
		assert_eq!(reader.get_container_name()?, "dummy container");
		assert_eq!(reader.get_name()?, "dummy name");
		assert_ne!(reader.get_parameters()?, &TileReaderParameters::new_dummy());
		assert_ne!(reader.get_parameters_mut()?, &mut TileReaderParameters::new_dummy());
		assert_eq!(reader.get_meta().await?, Some(Blob::from("dummy meta data")));
		let blob = reader
			.get_tile_data_original(&TileCoord3::new(0, 0, 0))
			.await
			.unwrap()
			.as_vec();
		assert_eq!(&blob[0..4], b"\x89PNG");
		Ok(())
	}

	#[tokio::test]
	async fn get_tile_data_original() {
		let test = |profile, blob| async move {
			let coord = TileCoord3::new(23, 45, 6);
			let mut reader = TileReader::new_dummy(profile, 8);
			let tile = reader.get_tile_data_original(&coord).await.unwrap();
			assert_eq!(tile, blob);
		};

		test(ReaderProfile::PNG, Blob::from(b"\x89PNG\x0d\x0a\x1a\x0a\x00\x00\x00\x0dIHDR\x00\x00\x01\x00\x00\x00\x01\x00\x01\x03\x00\x00\x00f\xbc:%\x00\x00\x00\x03PLTE\xaa\xd3\xdf\xcf\xec\xbc\xf5\x00\x00\x00\x1fIDATh\x81\xed\xc1\x01\x0d\x00\x00\x00\xc2\xa0\xf7Om\x0e7\xa0\x00\x00\x00\x00\x00\x00\x00\x00\xbe\x0d!\x00\x00\x01\x9a`\xe1\xd5\x00\x00\x00\x00IEND\xaeB`\x82".to_vec())).await;
		test(ReaderProfile::PBF, Blob::from(b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\xff\x016\x00\xc9\xff\x1a4\x0a\x05ocean\x12\x19\x12\x04\x00\x00\x01\x00\x18\x03\x22\x0f\x09)\xa8@\x1a\x00\xd1@\xd2@\x00\x00\xd2@\x0f\x1a\x01x\x1a\x01y\x22\x05\x15\x00\x00\x00\x00(\x80 x\x02C!\x1f_6\x00\x00\x00".to_vec())).await;
		test(ReaderProfile::JSON, Blob::from("{x:23,y:45,z:6}")).await;
	}

	#[tokio::test]
	async fn convert_from() {
		let mut converter = TileConverter::new_dummy(ConverterProfile::Png, 8);
		let mut reader = TileReader::new_dummy(ReaderProfile::PNG, 8);
		converter.convert_from(&mut reader).await.unwrap();
	}
}
