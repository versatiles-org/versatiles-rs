//! `converter` module provides functionalities to convert tile data between different formats and compressions.
//!
//! # Example Usage
//!
//! ```rust
//! use versatiles_container::*;
//! use versatiles_core::*;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let path_mbtiles = std::env::current_dir()?.join("../testdata/berlin.mbtiles");
//!     let path_versatiles = std::env::temp_dir().join("temp2.versatiles");
//!
//!     let registry = ContainerRegistry::default();
//!
//!     // Create a mbtiles reader
//!     let mut reader = registry.get_reader(&path_mbtiles.to_str().unwrap()).await?;
//!
//!     // Define converter parameters
//!     let converter_params = TilesConverterParameters {
//!         bbox_pyramid: Some(TileBBoxPyramid::new_full(8)),
//!         ..Default::default()
//!     };
//!
//!     // Convert the tiles container
//!     convert_tiles_container(
//!         reader,
//!         converter_params,
//!         &path_versatiles.to_str().unwrap(),
//!         registry
//!     ).await?;
//!
//!     println!("Tiles have been successfully converted and saved to {path_versatiles:?}");
//!     Ok(())
//! }
//! ```

use crate::{ContainerRegistry, Tile, TilesReaderTrait};
use anyhow::Result;
use async_trait::async_trait;
use versatiles_core::{
	TileBBox, TileBBoxPyramid, TileCompression, TileCoord, TileJSON, TileStream, TilesReaderParameters, Traversal,
};
use versatiles_derive::context;

/// Parameters for tile conversion.
#[derive(Debug)]
pub struct TilesConverterParameters {
	pub bbox_pyramid: Option<TileBBoxPyramid>,
	pub tile_compression: Option<TileCompression>,
	pub flip_y: bool,
	pub swap_xy: bool,
}

impl Default for TilesConverterParameters {
	/// Returns default converter parameters.
	fn default() -> Self {
		TilesConverterParameters {
			bbox_pyramid: None,
			tile_compression: None,
			flip_y: false,
			swap_xy: false,
		}
	}
}

/// Converts tiles from a given reader and writes them to a file.
#[context("Converting tiles from reader to file")]
pub async fn convert_tiles_container(
	reader: Box<dyn TilesReaderTrait>,
	cp: TilesConverterParameters,
	filename: &str,
	registry: ContainerRegistry,
) -> Result<()> {
	let converter = TilesConvertReader::new_from_reader(reader, cp)?;
	registry.write_to_filename(Box::new(converter), filename).await
}

/// A reader that converts tiles from one format to another.
#[derive(Debug)]
pub struct TilesConvertReader {
	reader: Box<dyn TilesReaderTrait>,
	converter_parameters: TilesConverterParameters,
	reader_parameters: TilesReaderParameters,
	container_name: String,
	name: String,
	tilejson: TileJSON,
}

impl TilesConvertReader {
	/// Creates a new converter reader from an existing reader.
	#[context("Creating converter reader from existing reader")]
	pub fn new_from_reader(
		reader: Box<dyn TilesReaderTrait>,
		cp: TilesConverterParameters,
	) -> Result<TilesConvertReader> {
		let container_name = format!("converter({})", reader.container_name());
		let name = format!("converter({})", reader.source_name());

		let rp: TilesReaderParameters = reader.parameters().to_owned();
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

		if let Some(tile_compression) = cp.tile_compression {
			new_rp.tile_compression = tile_compression;
		}

		let mut tilejson = reader.tilejson().clone();
		tilejson.update_from_reader_parameters(&new_rp);

		Ok(TilesConvertReader {
			reader,
			converter_parameters: cp,
			reader_parameters: new_rp,
			container_name,
			name,
			tilejson,
		})
	}
}

#[async_trait]
impl TilesReaderTrait for TilesConvertReader {
	fn source_name(&self) -> &str {
		&self.name
	}

	fn container_name(&self) -> &str {
		&self.container_name
	}

	fn traversal(&self) -> &Traversal {
		self.reader.traversal()
	}

	fn parameters(&self) -> &TilesReaderParameters {
		&self.reader_parameters
	}

	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.reader.override_compression(tile_compression);
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		let mut coord = *coord;

		if self.converter_parameters.flip_y {
			coord.flip_y();
		}

		if self.converter_parameters.swap_xy {
			coord.swap_xy();
		}

		let tile = self.reader.get_tile(&coord).await?;

		let mut tile = if let Some(tile) = tile { tile } else { return Ok(None) };

		if let Some(compression) = self.converter_parameters.tile_compression {
			tile.change_compression(compression)?;
		}

		Ok(Some(tile))
	}

	async fn get_tile_stream(&self, mut bbox: TileBBox) -> Result<TileStream<Tile>> {
		if self.converter_parameters.swap_xy {
			bbox.swap_xy();
		}
		if self.converter_parameters.flip_y {
			bbox.flip_y();
		}

		let mut stream = self.reader.get_tile_stream(bbox).await?;

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

		if let Some(tile_compression) = self.converter_parameters.tile_compression {
			stream = stream.map_item_parallel(move |mut tile| {
				tile.change_compression(tile_compression)?;
				Ok(tile)
			});
		}

		Ok(stream)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MockTilesReader, VersaTilesReader};
	use assert_fs::NamedTempFile;
	use versatiles_core::{
		TileCompression::*,
		TileFormat::{self, *},
	};

	fn get_mock_reader(tf: TileFormat, tc: TileCompression) -> MockTilesReader {
		let bbox_pyramid = TileBBoxPyramid::new_full(4);
		let reader_parameters = TilesReaderParameters::new(tf, tc, bbox_pyramid);
		MockTilesReader::new_mock(reader_parameters).unwrap()
	}

	#[tokio::test]
	async fn bbox_and_tile_order() -> Result<()> {
		test(false, false, [2, 3, 4, 5], "23 33 43 24 34 44 25 35 45").await?;
		test(false, true, [2, 3, 5, 4], "32 33 34 35 42 43 44 45").await?;
		test(true, false, [2, 3, 4, 6], "24 34 44 23 33 43 22 32 42 21 31 41").await?;
		test(true, true, [2, 3, 6, 4], "35 34 33 32 31 45 44 43 42 41").await?;

		async fn test(flip_y: bool, swap_xy: bool, bbox_out: [u32; 4], tile_list: &str) -> Result<()> {
			let pyramid_in = new_bbox([0, 1, 4, 5]);
			let pyramid_convert = new_bbox([2, 3, 7, 7]);
			let pyramid_out = new_bbox(bbox_out);

			let reader_parameters = TilesReaderParameters::new(JSON, Uncompressed, pyramid_in);
			let reader = MockTilesReader::new_mock(reader_parameters)?;

			let temp_file = NamedTempFile::new("test.versatiles")?;
			let filename = temp_file.to_str().unwrap();

			let cp = TilesConverterParameters {
				bbox_pyramid: Some(pyramid_convert),
				flip_y,
				swap_xy,
				tile_compression: None,
			};
			convert_tiles_container(reader.boxed(), cp, filename, ContainerRegistry::default()).await?;

			let reader_out = VersaTilesReader::open_path(&temp_file).await?;
			let parameters_out = reader_out.parameters();
			let tile_compression_out = parameters_out.tile_compression;
			assert_eq!(parameters_out.bbox_pyramid, pyramid_out);

			let bbox = pyramid_out.get_level_bbox(3);
			let mut tiles: Vec<String> = Vec::new();
			for coord in bbox.iter_coords() {
				let mut text = reader_out
					.get_tile(&coord)
					.await?
					.unwrap()
					.into_blob(tile_compression_out)?
					.to_string();
				text = text.replace("{x:", "").replace(",y:", "").replace(",z:3}", "");
				tiles.push(text);
			}
			let tiles = tiles.join(" ");
			assert_eq!(tiles, tile_list);

			Ok(())
		}

		fn new_bbox(b: [u32; 4]) -> TileBBoxPyramid {
			let mut pyramid = TileBBoxPyramid::new_empty();
			pyramid.include_bbox(&TileBBox::from_min_and_max(3, b[0], b[1], b[2], b[3]).unwrap());
			pyramid
		}

		Ok(())
	}

	#[test]
	fn test_tiles_converter_parameters_new() {
		let cp = TilesConverterParameters {
			bbox_pyramid: Some(TileBBoxPyramid::new_full(1)),
			flip_y: true,
			swap_xy: true,
			tile_compression: None,
		};

		assert!(cp.bbox_pyramid.is_some());
		assert!(cp.flip_y);
		assert!(cp.swap_xy);
	}

	#[test]
	fn test_tiles_converter_parameters_default() {
		let cp = TilesConverterParameters::default();

		assert_eq!(cp.bbox_pyramid, None);
		assert!(!cp.flip_y);
		assert!(!cp.swap_xy);
	}

	#[test]
	fn test_tiles_convert_reader_new_from_reader() {
		let reader = get_mock_reader(MVT, Uncompressed);
		let cp = TilesConverterParameters::default();

		let tcr = TilesConvertReader::new_from_reader(reader.boxed(), cp).unwrap();

		assert_eq!(tcr.reader.container_name(), "dummy_container");
		assert_eq!(tcr.name, "converter(dummy_name)");
		assert_eq!(tcr.container_name, "converter(dummy_container)");
	}

	#[tokio::test]
	async fn test_get_tile() -> Result<()> {
		let reader = get_mock_reader(MVT, Uncompressed);
		let cp = TilesConverterParameters::default();
		let tcr = TilesConvertReader::new_from_reader(reader.boxed(), cp)?;

		let coord = TileCoord::new(0, 0, 0)?;
		let data = tcr.get_tile(&coord).await?;
		assert!(data.is_some());

		Ok(())
	}

	#[test]
	fn test_get_name() {
		let reader = get_mock_reader(MVT, Uncompressed);
		let cp = TilesConverterParameters::default();
		let tcr = TilesConvertReader::new_from_reader(reader.boxed(), cp).unwrap();

		assert_eq!(tcr.source_name(), "converter(dummy_name)");
	}

	#[test]
	fn test_container_name() {
		let reader = get_mock_reader(MVT, Uncompressed);
		let cp = TilesConverterParameters::default();
		let tcr = TilesConvertReader::new_from_reader(reader.boxed(), cp).unwrap();

		assert_eq!(tcr.container_name(), "converter(dummy_container)");
	}

	#[test]
	fn test_override_compression() {
		let reader = get_mock_reader(MVT, Uncompressed);
		let cp = TilesConverterParameters::default();
		let mut tcr = TilesConvertReader::new_from_reader(reader.boxed(), cp).unwrap();

		tcr.override_compression(Gzip);
		assert_eq!(tcr.reader.parameters().tile_compression, Gzip);
	}

	#[tokio::test]
	async fn test_flip_y_and_swap_xy() -> Result<()> {
		let reader = get_mock_reader(MVT, Uncompressed);
		let cp = TilesConverterParameters {
			flip_y: true,
			swap_xy: true,
			..Default::default()
		};
		let tcr = TilesConvertReader::new_from_reader(reader.boxed(), cp)?;

		let mut coord = TileCoord::new(4, 5, 6)?;
		let data = tcr.get_tile(&coord).await?;
		assert!(data.is_some());

		coord.flip_y();
		coord.swap_xy();
		let data_flipped = tcr.get_tile(&coord).await?;
		assert_eq!(data, data_flipped);

		Ok(())
	}
}
