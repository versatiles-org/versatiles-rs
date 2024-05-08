use super::{get_writer, TilesReaderParameters, TilesWriterParameters};
use crate::shared::transform_coord::TransformCoord;
use crate::shared::DataConverter;
use crate::{
	containers::{TilesReaderBox, TilesReaderTrait, TilesStream},
	shared::{Blob, Compression, TileBBox, TileBBoxPyramid, TileCoord3, TileFormat},
};
use anyhow::Result;
use async_trait::async_trait;

#[derive(Debug)]
pub struct TileConverterParameters {
	tile_format: Option<TileFormat>,
	tile_compression: Option<Compression>,
	bbox_pyramid: Option<TileBBoxPyramid>,
	force_recompress: bool,
	flip_y: bool,
	swap_xy: bool,
}

pub async fn convert(reader: TilesReaderBox, cp: TileConverterParameters, filename: &str) -> Result<()> {
	let rp = reader.get_parameters();

	let wp = TilesWriterParameters {
		tile_format: cp.tile_format.unwrap_or(rp.tile_format),
		tile_compression: cp.tile_compression.unwrap_or(rp.tile_compression),
	};
	let mut writer = get_writer(filename, wp).await?;

	let mut converter = TileConvertReader::new(reader, cp)?;
	writer.write_from_reader(&mut converter).await
}

#[derive(Debug)]
struct TileConvertReader {
	reader: TilesReaderBox,
	converter_parameters: TileConverterParameters,
	reader_parameters: TilesReaderParameters,
	container_name: String,
	tile_recompressor: Option<DataConverter>,
	name: String,
}

impl TileConvertReader {
	pub fn new(reader: TilesReaderBox, cp: TileConverterParameters) -> Result<TilesReaderBox> {
		let container_name = format!("converter({})", reader.get_container_name());
		let name = format!("converter({})", reader.get_name());

		let rp: TilesReaderParameters = *reader.get_parameters();
		let mut new_rp: TilesReaderParameters = rp.clone();

		if cp.flip_y {
			new_rp.bbox_pyramid.flip_y();
		}
		if cp.swap_xy {
			new_rp.bbox_pyramid.swap_xy();
		}

		if cp.bbox_pyramid.is_some() {
			new_rp.bbox_pyramid.intersect(cp.bbox_pyramid.as_ref().unwrap());
		}

		new_rp.tile_format = cp.tile_format.unwrap_or(rp.tile_format);
		new_rp.tile_compression = cp.tile_compression.unwrap_or(rp.tile_compression);

		let tile_recompressor = Some(DataConverter::new_tile_recompressor(
			&rp.tile_format,
			&rp.tile_compression,
			&new_rp.tile_format,
			&new_rp.tile_compression,
			cp.force_recompress,
		)?);

		Ok(Box::new(TileConvertReader {
			reader,
			converter_parameters: cp,
			reader_parameters: new_rp,
			container_name,
			tile_recompressor,
			name,
		}))
	}
}

#[async_trait]
impl TilesReaderTrait for TileConvertReader {
	fn get_name(&self) -> &str {
		&self.name
	}

	fn get_container_name(&self) -> &str {
		&self.container_name
	}

	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.reader_parameters
	}

	async fn get_meta(&self) -> Result<Option<Blob>> {
		self.reader.get_meta().await
	}

	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Blob> {
		let coord = &mut coord.clone();
		if self.converter_parameters.flip_y {
			coord.flip_y();
		}
		if self.converter_parameters.swap_xy {
			coord.swap_xy();
		}
		let mut blob = self.reader.get_tile_data(coord).await?;

		if self.tile_recompressor.is_some() {
			blob = self.tile_recompressor.as_ref().unwrap().process_blob(blob)?;
		}

		Ok(blob)
	}

	async fn get_bbox_tile_stream<'a>(&'a mut self, bbox: TileBBox) -> TilesStream {
		let mut bbox: TileBBox = bbox.clone();
		if self.converter_parameters.flip_y {
			bbox.flip_y();
		}
		if self.converter_parameters.swap_xy {
			bbox.swap_xy();
		}
		let mut stream = self.reader.get_bbox_tile_stream(bbox).await;

		if self.tile_recompressor.is_some() {
			stream = self.tile_recompressor.as_ref().unwrap().process_stream(stream);
		}

		stream
	}
}

#[cfg(test)]
mod tests {
	use assert_fs::NamedTempFile;

	use crate::containers::{MockTilesReader, VersaTilesReader};

	use super::*;

	fn get_mock_reader(tf: TileFormat, tc: Compression) -> TilesReaderBox {
		let bbox_pyramid = TileBBoxPyramid::new_full(2);
		let reader_parameters = TilesReaderParameters::new(tf, tc, bbox_pyramid);
		MockTilesReader::new_mock(reader_parameters)
	}
	fn get_converter_parameters(tf: TileFormat, tc: Compression, force_recompress: bool) -> TileConverterParameters {
		TileConverterParameters {
			tile_format: Some(tf),
			tile_compression: Some(tc),
			bbox_pyramid: None,
			force_recompress,
			flip_y: false,
			swap_xy: false,
		}
	}

	#[tokio::test]
	async fn test_recompression() -> Result<()> {
		async fn test(c_in: Compression, c_out: Compression) -> Result<()> {
			let reader_in = get_mock_reader(TileFormat::PBF, c_in);
			let temp_file = NamedTempFile::new("test.versatiles")?;
			let cp = get_converter_parameters(TileFormat::PBF, c_out, false);
			let filename = temp_file.to_str().unwrap();
			convert(reader_in, cp, filename).await?;
			let reader_out = VersaTilesReader::open_file(&temp_file).await?;
			let parameters_out = reader_out.get_parameters();
			assert_eq!(parameters_out.tile_format, TileFormat::PBF);
			assert_eq!(parameters_out.tile_compression, c_out);
			Ok(())
		}

		test(Compression::None, Compression::None).await?;
		test(Compression::None, Compression::Gzip).await?;
		test(Compression::None, Compression::Brotli).await?;
		test(Compression::Gzip, Compression::None).await?;
		test(Compression::Gzip, Compression::Gzip).await?;
		test(Compression::Gzip, Compression::Brotli).await?;
		test(Compression::Brotli, Compression::None).await?;
		test(Compression::Brotli, Compression::Gzip).await?;
		test(Compression::Brotli, Compression::Brotli).await?;

		Ok(())
	}

	#[tokio::test]
	async fn test_conversion() -> Result<()> {
		async fn test(f_in: TileFormat, f_out: TileFormat) -> Result<()> {
			let reader_in = get_mock_reader(f_in, Compression::Gzip);
			let temp_file = NamedTempFile::new("test.versatiles")?;
			let cp = get_converter_parameters(f_out, Compression::Gzip, false);
			let filename = temp_file.to_str().unwrap();
			convert(reader_in, cp, filename).await?;
			let reader_out = VersaTilesReader::open_file(&temp_file).await?;
			let parameters_out = reader_out.get_parameters();
			assert_eq!(parameters_out.tile_format, f_out);
			assert_eq!(parameters_out.tile_compression, Compression::Gzip);
			Ok(())
		}

		test(TileFormat::PNG, TileFormat::PNG).await?;

		test(TileFormat::PNG, TileFormat::WEBP).await?;
		test(TileFormat::PNG, TileFormat::JPG).await?;
		test(TileFormat::PNG, TileFormat::AVIF).await?;

		test(TileFormat::WEBP, TileFormat::PNG).await?;
		test(TileFormat::JPG, TileFormat::PNG).await?;
		test(TileFormat::AVIF, TileFormat::PNG).await?;

		Ok(())
	}
}
