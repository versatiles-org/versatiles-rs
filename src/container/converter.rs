use crate::{
	container::{
		get_writer, TilesReaderBox, TilesReaderParameters, TilesReaderTrait, TilesStream, TilesWriterParameters,
	},
	helper::{TileConverter, TransformCoord},
	types::{Blob, TileBBox, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat},
};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;

#[derive(Debug)]
pub struct TilesConverterParameters {
	pub tile_format: Option<TileFormat>,
	pub tile_compression: Option<TileCompression>,
	pub bbox_pyramid: Option<TileBBoxPyramid>,
	pub force_recompress: bool,
	pub flip_y: bool,
	pub swap_xy: bool,
}

impl TilesConverterParameters {
	pub fn new(
		tile_format: Option<TileFormat>, tile_compression: Option<TileCompression>,
		bbox_pyramid: Option<TileBBoxPyramid>, force_recompress: bool, flip_y: bool, swap_xy: bool,
	) -> TilesConverterParameters {
		TilesConverterParameters {
			tile_format,
			tile_compression,
			bbox_pyramid,
			force_recompress,
			flip_y,
			swap_xy,
		}
	}
	pub fn new_default() -> TilesConverterParameters {
		TilesConverterParameters {
			tile_format: None,
			tile_compression: None,
			bbox_pyramid: None,
			force_recompress: false,
			flip_y: false,
			swap_xy: false,
		}
	}
}

pub async fn convert_tiles_container(
	reader: TilesReaderBox, cp: TilesConverterParameters, filename: &str,
) -> Result<()> {
	let rp = reader.get_parameters();

	let wp = TilesWriterParameters {
		tile_format: cp.tile_format.unwrap_or(rp.tile_format),
		tile_compression: cp.tile_compression.unwrap_or(rp.tile_compression),
	};
	let mut writer = get_writer(filename, wp).await?;

	let mut converter = TilesConvertReader::new_from_reader(reader, cp)?;
	writer.write_from_reader(&mut converter).await
}

#[derive(Debug)]
pub struct TilesConvertReader {
	reader: TilesReaderBox,
	converter_parameters: TilesConverterParameters,
	reader_parameters: TilesReaderParameters,
	container_name: String,
	tile_recompressor: Option<TileConverter>,
	name: String,
}

impl TilesConvertReader {
	pub fn new_from_reader(reader: TilesReaderBox, cp: TilesConverterParameters) -> Result<TilesReaderBox> {
		let container_name = format!("converter({})", reader.get_container_name());
		let name = format!("converter({})", reader.get_name());

		let rp: TilesReaderParameters = reader.get_parameters().to_owned();
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

		let tile_recompressor = Some(TileConverter::new_tile_recompressor(
			&rp.tile_format,
			&rp.tile_compression,
			&new_rp.tile_format,
			&new_rp.tile_compression,
			cp.force_recompress,
		)?);

		Ok(Box::new(TilesConvertReader {
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
impl TilesReaderTrait for TilesConvertReader {
	fn get_name(&self) -> &str {
		&self.name
	}

	fn get_container_name(&self) -> &str {
		&self.container_name
	}

	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.reader_parameters
	}

	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.reader.override_compression(tile_compression);
	}

	async fn get_meta(&self) -> Result<Option<Blob>> {
		self.reader.get_meta().await
	}

	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		let coord = &mut coord.clone();
		if self.converter_parameters.flip_y {
			coord.flip_y();
		}
		if self.converter_parameters.swap_xy {
			coord.swap_xy();
		}
		let blob = self.reader.get_tile_data(coord).await?;

		if blob.is_none() {
			return Ok(None);
		}
		let mut blob = blob.unwrap();

		if self.tile_recompressor.is_some() {
			blob = self.tile_recompressor.as_ref().unwrap().process_blob(blob)?
		}

		Ok(Some(blob))
	}

	async fn get_bbox_tile_stream<'a>(&'a mut self, bbox: &TileBBox) -> TilesStream {
		let mut bbox: TileBBox = bbox.clone();
		if self.converter_parameters.swap_xy {
			bbox.swap_xy();
		}
		if self.converter_parameters.flip_y {
			bbox.flip_y();
		}

		let mut stream = self.reader.get_bbox_tile_stream(&bbox).await;

		let flip_y = self.converter_parameters.flip_y;
		let swap_xy = self.converter_parameters.swap_xy;

		if flip_y || swap_xy {
			stream = stream
				.map(move |(mut coord, blob)| {
					if flip_y {
						coord.flip_y()
					}
					if swap_xy {
						coord.swap_xy()
					}
					(coord, blob)
				})
				.boxed()
		}

		if self.tile_recompressor.is_some() {
			stream = self.tile_recompressor.as_ref().unwrap().process_stream(stream);
		}

		stream
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::container::{MockTilesReader, VersaTilesReader};
	use assert_fs::NamedTempFile;

	fn get_mock_reader(tf: TileFormat, tc: TileCompression) -> TilesReaderBox {
		let bbox_pyramid = TileBBoxPyramid::new_full(1);
		let reader_parameters = TilesReaderParameters::new(tf, tc, bbox_pyramid);
		MockTilesReader::new_mock(reader_parameters)
	}
	fn get_converter_parameters(
		tf: TileFormat, tc: TileCompression, force_recompress: bool,
	) -> TilesConverterParameters {
		TilesConverterParameters {
			tile_format: Some(tf),
			tile_compression: Some(tc),
			bbox_pyramid: None,
			force_recompress,
			flip_y: false,
			swap_xy: false,
		}
	}

	#[tokio::test]
	async fn tile_recompression() -> Result<()> {
		use TileCompression::*;

		async fn test(c_in: TileCompression, c_out: TileCompression) -> Result<()> {
			let reader_in = get_mock_reader(TileFormat::PBF, c_in);
			let temp_file = NamedTempFile::new("test.versatiles")?;
			let cp = get_converter_parameters(TileFormat::PBF, c_out, false);
			let filename = temp_file.to_str().unwrap();
			convert_tiles_container(reader_in, cp, filename).await?;
			let reader_out = VersaTilesReader::open_file(&temp_file).await?;
			let parameters_out = reader_out.get_parameters();
			assert_eq!(parameters_out.tile_format, TileFormat::PBF);
			assert_eq!(parameters_out.tile_compression, c_out);
			Ok(())
		}

		test(None, None).await?;
		test(None, Gzip).await?;
		test(None, Brotli).await?;
		test(Gzip, None).await?;
		test(Gzip, Gzip).await?;
		test(Gzip, Brotli).await?;
		test(Brotli, None).await?;
		test(Brotli, Gzip).await?;
		test(Brotli, Brotli).await?;

		Ok(())
	}

	#[tokio::test]
	async fn tile_conversion() -> Result<()> {
		use TileFormat::*;

		async fn test(f_in: TileFormat, f_out: TileFormat) -> Result<()> {
			let reader_in = get_mock_reader(f_in, TileCompression::Gzip);
			let temp_file = NamedTempFile::new("test.versatiles")?;
			let cp = get_converter_parameters(f_out, TileCompression::Gzip, false);
			let filename = temp_file.to_str().unwrap();
			convert_tiles_container(reader_in, cp, filename).await?;
			let reader_out = VersaTilesReader::open_file(&temp_file).await?;
			let parameters_out = reader_out.get_parameters();
			assert_eq!(parameters_out.tile_format, f_out);
			assert_eq!(parameters_out.tile_compression, TileCompression::Gzip);
			Ok(())
		}

		test(PNG, PNG).await?;

		test(PNG, WEBP).await?;
		test(PNG, JPG).await?;
		test(PNG, AVIF).await.unwrap_err();

		test(WEBP, PNG).await?;
		test(PNG, WEBP).await?;
		test(JPG, PNG).await?;
		test(AVIF, PNG).await.unwrap_err();

		test(PNG, PBF).await.unwrap_err();
		test(PBF, PNG).await.unwrap_err();

		Ok(())
	}

	#[tokio::test]
	async fn bbox_and_tile_order() -> Result<()> {
		use TileCompression::None;
		use TileFormat::JSON;

		test(false, false, [2, 3, 4, 5], "23 33 43 24 34 44 25 35 45").await?;
		test(false, true, [2, 3, 5, 4], "32 33 34 35 42 43 44 45").await?;
		test(true, false, [2, 3, 4, 6], "24 34 44 23 33 43 22 32 42 21 31 41").await?;
		test(true, true, [2, 3, 6, 4], "35 34 33 32 31 45 44 43 42 41").await?;

		async fn test(flip_y: bool, swap_xy: bool, bbox_out: [u32; 4], tile_list: &str) -> Result<()> {
			let pyramid_in = new_bbox([0, 1, 4, 5]);
			let pyramid_convert = new_bbox([2, 3, 7, 7]);
			let pyramid_out = new_bbox(bbox_out);

			let reader_parameters = TilesReaderParameters::new(JSON, None, pyramid_in);
			let reader = MockTilesReader::new_mock(reader_parameters);

			let temp_file = NamedTempFile::new("test.versatiles")?;
			let filename = temp_file.to_str().unwrap();

			let cp = TilesConverterParameters::new(Some(JSON), Some(None), Some(pyramid_convert), false, flip_y, swap_xy);
			convert_tiles_container(reader, cp, filename).await?;

			let mut reader_out = VersaTilesReader::open_file(&temp_file).await?;
			let parameters_out = reader_out.get_parameters();
			assert_eq!(parameters_out.bbox_pyramid, pyramid_out);

			let bbox = pyramid_out.get_level_bbox(3);
			let mut tiles: Vec<String> = Vec::new();
			for coord in bbox.iter_coords() {
				let mut text = reader_out.get_tile_data(&coord).await?.unwrap().to_string();
				text = text.replace("{x:", "").replace(",y:", "").replace(",z:3}", "");
				tiles.push(text);
			}
			let tiles = tiles.join(" ");
			assert_eq!(tiles, tile_list);

			Ok(())
		}

		fn new_bbox(b: [u32; 4]) -> TileBBoxPyramid {
			let mut pyramid = TileBBoxPyramid::new_empty();
			pyramid.include_bbox(&TileBBox::new(3, b[0], b[1], b[2], b[3]).unwrap());
			pyramid
		}

		Ok(())
	}
}
