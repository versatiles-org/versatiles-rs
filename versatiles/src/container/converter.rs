//! `converter` module provides functionalities to convert tile data between different formats and compressions.
//!
//! # Example Usage
//!
//! ```rust
//! use versatiles::container::{convert_tiles_container, MBTilesReader, TilesConverterParameters};
//! use versatiles::types::{TileFormat, TileCompression, TileBBoxPyramid, TilesReader, TilesReaderParameters};
//! use std::path::Path;
//! use anyhow::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let path_mbtiles = std::env::current_dir()?.join("../testdata/berlin.mbtiles");
//!     let path_versatiles = std::env::current_dir()?.join("../testdata/temp2.versatiles");
//!
//!     // Create a mbtiles reader
//!     let mut reader = MBTilesReader::open_path(&path_mbtiles)?;
//!
//!     // Define converter parameters
//!     let converter_params = TilesConverterParameters::new(
//!         None,
//!         Some(TileCompression::Brotli),
//!         Some(TileBBoxPyramid::new_full(8)),
//!         false,
//!         false,
//!         false,
//!     );
//!
//!     // Convert the tiles container
//!     convert_tiles_container(Box::new(reader), converter_params, &path_versatiles.to_str().unwrap()).await?;
//!
//!     println!("Tiles have been successfully converted and saved to {path_versatiles:?}");
//!     Ok(())
//! }
//! ```

use super::{utils::TileConverter, write_to_filename};
use crate::{
	types::{
		Blob, TileBBox, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat, TileStream,
		TilesReader, TilesReaderParameters,
	},
	utils::TransformCoord,
};
use anyhow::Result;
use async_trait::async_trait;

/// Parameters for tile conversion.
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
	/// Create new converter parameters with specific settings.
	pub fn new(
		tile_format: Option<TileFormat>,
		tile_compression: Option<TileCompression>,
		bbox_pyramid: Option<TileBBoxPyramid>,
		force_recompress: bool,
		flip_y: bool,
		swap_xy: bool,
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

	/// Create new converter parameters with default settings.
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

/// Converts tiles from a given reader and writes them to a file.
pub async fn convert_tiles_container(
	reader: Box<dyn TilesReader>,
	cp: TilesConverterParameters,
	filename: &str,
) -> Result<()> {
	let mut converter = TilesConvertReader::new_from_reader(reader, cp)?;
	write_to_filename(&mut converter, filename).await
}

/// A reader that converts tiles from one format to another.
#[derive(Debug)]
pub struct TilesConvertReader {
	reader: Box<dyn TilesReader>,
	converter_parameters: TilesConverterParameters,
	reader_parameters: TilesReaderParameters,
	container_name: String,
	tile_recompressor: Option<TileConverter>,
	name: String,
}

impl TilesConvertReader {
	/// Creates a new converter reader from an existing reader.
	pub fn new_from_reader(
		reader: Box<dyn TilesReader>,
		cp: TilesConverterParameters,
	) -> Result<TilesConvertReader> {
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

		if let Some(bbox_pyramid) = &cp.bbox_pyramid {
			new_rp.bbox_pyramid.intersect(bbox_pyramid);
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

		Ok(TilesConvertReader {
			reader,
			converter_parameters: cp,
			reader_parameters: new_rp,
			container_name,
			tile_recompressor,
			name,
		})
	}
}

#[async_trait]
impl TilesReader for TilesConvertReader {
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

	fn get_meta(&self) -> Result<Option<Blob>> {
		self.reader.get_meta()
	}

	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		let mut coord = *coord;
		if self.converter_parameters.flip_y {
			coord.flip_y();
		}
		if self.converter_parameters.swap_xy {
			coord.swap_xy();
		}
		let mut blob = self.reader.get_tile_data(&coord).await?;

		if let Some(tile_recompressor) = &self.tile_recompressor {
			if let Some(b) = blob {
				blob = Some(tile_recompressor.process_blob(b)?);
			}
		}

		Ok(blob)
	}

	async fn get_bbox_tile_stream(&mut self, bbox: TileBBox) -> TileStream {
		let mut bbox = bbox.clone();
		if self.converter_parameters.swap_xy {
			bbox.swap_xy();
		}
		if self.converter_parameters.flip_y {
			bbox.flip_y();
		}

		let mut stream = self.reader.get_bbox_tile_stream(bbox).await;

		let flip_y = self.converter_parameters.flip_y;
		let swap_xy = self.converter_parameters.swap_xy;

		if flip_y || swap_xy {
			stream = stream.map_coord(move |mut coord| {
				if flip_y {
					coord.flip_y()
				}
				if swap_xy {
					coord.swap_xy()
				}
				coord
			});
		}

		if let Some(tile_recompressor) = &self.tile_recompressor {
			stream = tile_recompressor.process_stream(stream);
		}

		stream
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::container::{MockTilesReader, VersaTilesReader};
	use assert_fs::NamedTempFile;
	use TileCompression::*;
	use TileFormat::*;

	fn get_mock_reader(tf: TileFormat, tc: TileCompression) -> MockTilesReader {
		let bbox_pyramid = TileBBoxPyramid::new_full(1);
		let reader_parameters = TilesReaderParameters::new(tf, tc, bbox_pyramid);
		MockTilesReader::new_mock(reader_parameters).unwrap()
	}

	fn get_converter_parameters(
		tf: TileFormat,
		tc: TileCompression,
		force_recompress: bool,
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
		async fn test(c_in: TileCompression, c_out: TileCompression) -> Result<()> {
			let reader_in = get_mock_reader(PBF, c_in);
			let temp_file = NamedTempFile::new("test.versatiles")?;
			let cp = get_converter_parameters(PBF, c_out, false);
			let filename = temp_file.to_str().unwrap();
			convert_tiles_container(reader_in.boxed(), cp, filename).await?;
			let reader_out = VersaTilesReader::open_path(&temp_file).await?;
			let parameters_out = reader_out.get_parameters();
			assert_eq!(parameters_out.tile_format, PBF);
			assert_eq!(parameters_out.tile_compression, c_out);
			Ok(())
		}

		test(Uncompressed, Uncompressed).await?;
		test(Uncompressed, Gzip).await?;
		test(Uncompressed, Brotli).await?;
		test(Gzip, Uncompressed).await?;
		test(Gzip, Gzip).await?;
		test(Gzip, Brotli).await?;
		test(Brotli, Uncompressed).await?;
		test(Brotli, Gzip).await?;
		test(Brotli, Brotli).await?;

		Ok(())
	}

	#[tokio::test]
	async fn tile_conversion() -> Result<()> {
		async fn test(f_in: TileFormat, f_out: TileFormat) -> Result<()> {
			let reader_in = get_mock_reader(f_in, Gzip);
			let temp_file = NamedTempFile::new("test.versatiles")?;
			let cp = get_converter_parameters(f_out, Gzip, false);
			let filename = temp_file.to_str().unwrap();
			convert_tiles_container(reader_in.boxed(), cp, filename).await?;
			let reader_out = VersaTilesReader::open_path(&temp_file).await?;
			let parameters_out = reader_out.get_parameters();
			assert_eq!(parameters_out.tile_format, f_out);
			assert_eq!(parameters_out.tile_compression, Gzip);
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
		test(false, false, [2, 3, 4, 5], "23 33 43 24 34 44 25 35 45").await?;
		test(false, true, [2, 3, 5, 4], "32 33 34 35 42 43 44 45").await?;
		test(
			true,
			false,
			[2, 3, 4, 6],
			"24 34 44 23 33 43 22 32 42 21 31 41",
		)
		.await?;
		test(true, true, [2, 3, 6, 4], "35 34 33 32 31 45 44 43 42 41").await?;

		async fn test(
			flip_y: bool,
			swap_xy: bool,
			bbox_out: [u32; 4],
			tile_list: &str,
		) -> Result<()> {
			let pyramid_in = new_bbox([0, 1, 4, 5]);
			let pyramid_convert = new_bbox([2, 3, 7, 7]);
			let pyramid_out = new_bbox(bbox_out);

			let reader_parameters = TilesReaderParameters::new(JSON, Uncompressed, pyramid_in);
			let reader = MockTilesReader::new_mock(reader_parameters)?;

			let temp_file = NamedTempFile::new("test.versatiles")?;
			let filename = temp_file.to_str().unwrap();

			let cp = TilesConverterParameters::new(
				Some(JSON),
				Some(Uncompressed),
				Some(pyramid_convert),
				false,
				flip_y,
				swap_xy,
			);
			convert_tiles_container(reader.boxed(), cp, filename).await?;

			let mut reader_out = VersaTilesReader::open_path(&temp_file).await?;
			let parameters_out = reader_out.get_parameters();
			assert_eq!(parameters_out.bbox_pyramid, pyramid_out);

			let bbox = pyramid_out.get_level_bbox(3);
			let mut tiles: Vec<String> = Vec::new();
			for coord in bbox.iter_coords() {
				let mut text = reader_out.get_tile_data(&coord).await?.unwrap().to_string();
				text = text
					.replace("{x:", "")
					.replace(",y:", "")
					.replace(",z:3}", "");
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

	#[test]
	fn test_tiles_converter_parameters_new() {
		let cp = TilesConverterParameters::new(
			Some(PNG),
			Some(Gzip),
			Some(TileBBoxPyramid::new_full(1)),
			true,
			true,
			true,
		);

		assert_eq!(cp.tile_format, Some(PNG));
		assert_eq!(cp.tile_compression, Some(Gzip));
		assert!(cp.bbox_pyramid.is_some());
		assert!(cp.force_recompress);
		assert!(cp.flip_y);
		assert!(cp.swap_xy);
	}

	#[test]
	fn test_tiles_converter_parameters_new_default() {
		let cp = TilesConverterParameters::new_default();

		assert_eq!(cp.tile_format, None);
		assert_eq!(cp.tile_compression, None);
		assert_eq!(cp.bbox_pyramid, None);
		assert!(!cp.force_recompress);
		assert!(!cp.flip_y);
		assert!(!cp.swap_xy);
	}

	#[test]
	fn test_tiles_convert_reader_new_from_reader() {
		let reader = get_mock_reader(PBF, Uncompressed);
		let cp = TilesConverterParameters::new_default();

		let tcr = TilesConvertReader::new_from_reader(reader.boxed(), cp).unwrap();

		assert_eq!(tcr.reader.get_container_name(), "dummy_container");
		assert_eq!(tcr.converter_parameters.tile_format, None);
		assert_eq!(tcr.converter_parameters.tile_compression, None);
		assert_eq!(tcr.name, "converter(dummy_name)");
		assert_eq!(tcr.container_name, "converter(dummy_container)");
	}

	#[tokio::test]
	async fn test_get_tile_data() -> Result<()> {
		let reader = get_mock_reader(PBF, Uncompressed);
		let cp = TilesConverterParameters::new_default();
		let mut tcr = TilesConvertReader::new_from_reader(reader.boxed(), cp)?;

		let coord = TileCoord3::new(0, 0, 0)?;
		let data = tcr.get_tile_data(&coord).await?;
		assert!(data.is_some());

		Ok(())
	}

	#[test]
	fn test_get_name() {
		let reader = get_mock_reader(PBF, Uncompressed);
		let cp = TilesConverterParameters::new_default();
		let tcr = TilesConvertReader::new_from_reader(reader.boxed(), cp).unwrap();

		assert_eq!(tcr.get_name(), "converter(dummy_name)");
	}

	#[test]
	fn test_get_container_name() {
		let reader = get_mock_reader(PBF, Uncompressed);
		let cp = TilesConverterParameters::new_default();
		let tcr = TilesConvertReader::new_from_reader(reader.boxed(), cp).unwrap();

		assert_eq!(tcr.get_container_name(), "converter(dummy_container)");
	}

	#[test]
	fn test_override_compression() {
		let reader = get_mock_reader(PBF, Uncompressed);
		let cp = TilesConverterParameters::new_default();
		let mut tcr = TilesConvertReader::new_from_reader(reader.boxed(), cp).unwrap();

		tcr.override_compression(Gzip);
		assert_eq!(tcr.reader.get_parameters().tile_compression, Gzip);
	}

	#[tokio::test]
	async fn test_flip_y_and_swap_xy() -> Result<()> {
		let reader = get_mock_reader(PBF, Uncompressed);
		let cp =
			TilesConverterParameters::new(Some(PBF), Some(Uncompressed), None, false, true, true);
		let mut tcr = TilesConvertReader::new_from_reader(reader.boxed(), cp)?;

		let mut coord = TileCoord3::new(1, 2, 3)?;
		let data = tcr.get_tile_data(&coord).await?;
		assert!(data.is_some());

		coord.flip_y();
		coord.swap_xy();
		let data_flipped = tcr.get_tile_data(&coord).await?;
		assert_eq!(data, data_flipped);

		Ok(())
	}
}
