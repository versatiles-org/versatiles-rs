use crate::{
	containers::{TileReaderBox, TileReaderTrait},
	shared::{
		compress_gzip, Blob, Compression, Error, Result, TileBBoxPyramide, TileCoord3, TileFormat, TileReaderParameters,
	},
};
use async_trait::async_trait;

#[derive(Debug)]
pub enum ReaderProfile {
	PngFast,
	PbfFast,
}

pub struct TileReader {
	parameters: TileReaderParameters,
	tile_blob: Blob,
}

impl TileReader {
	pub fn new_dummy(profile: ReaderProfile, max_zoom_level: u8) -> TileReaderBox {
		let mut bbox_pyramide = TileBBoxPyramide::new_full();
		bbox_pyramide.set_zoom_max(max_zoom_level);

		let parameters;
		let tile_blob;

		match profile {
			ReaderProfile::PngFast => {
				parameters = TileReaderParameters::new(TileFormat::PNG, Compression::None, bbox_pyramide);
				tile_blob = Blob::from(include_bytes!("./dummy.png").to_vec());
			}
			ReaderProfile::PbfFast => {
				parameters = TileReaderParameters::new(TileFormat::PBF, Compression::Gzip, bbox_pyramide);
				tile_blob = compress_gzip(Blob::from(include_bytes!("./dummy.pbf").to_vec())).unwrap();
			}
		};

		Box::new(Self { parameters, tile_blob })
	}
}

#[async_trait]
impl TileReaderTrait for TileReader {
	async fn new(_path: &str) -> Result<TileReaderBox> {
		Err(Error::new("don't want to"))
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
	async fn get_meta(&self) -> Result<Blob> {
		Ok(Blob::from("dummy meta data"))
	}
	async fn get_tile_data(&self, _coord: &TileCoord3) -> Option<Blob> {
		Some(self.tile_blob.clone())
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
		shared::{Blob, TileCoord3, TileReaderParameters},
	};
	use futures::executor::block_on;

	#[test]
	fn reader() {
		let mut reader = TileReader::new_dummy(ReaderProfile::PngFast, 8);
		assert_eq!(reader.get_container_name().unwrap(), "dummy container");
		assert_eq!(reader.get_name().unwrap(), "dummy name");
		assert_ne!(reader.get_parameters().unwrap(), &TileReaderParameters::new_dummy());
		assert_ne!(
			reader.get_parameters_mut().unwrap(),
			&mut TileReaderParameters::new_dummy()
		);
		assert_eq!(block_on(reader.get_meta()).unwrap(), Blob::from("dummy meta data"));
		let blob = block_on(reader.get_tile_data(&TileCoord3::new(0, 0, 0)))
			.unwrap()
			.as_vec();
		assert_eq!(&blob[0..4], b"\x89PNG");
	}

	#[test]
	fn test2() {
		let mut converter = TileConverter::new_dummy(ConverterProfile::Png, 8);
		let mut reader = TileReader::new_dummy(ReaderProfile::PngFast, 8);
		block_on(converter.convert_from(&mut reader)).unwrap();
	}
}
