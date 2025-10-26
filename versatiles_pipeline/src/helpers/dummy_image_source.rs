use crate::OperationTrait;
use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use imageproc::image::DynamicImage;
use nom::Input;
use versatiles_container::{Tile, TilesReaderTrait};
use versatiles_core::*;
use versatiles_derive::context;
use versatiles_image::traits::*;

#[derive(Debug)]
pub struct DummyImageSource {
	#[allow(clippy::type_complexity)]
	tile: Tile,
	parameters: TilesReaderParameters,
	tilejson: TileJSON,
}

impl DummyImageSource {
	#[allow(clippy::type_complexity)]
	#[context("Creating DummyImageSource for {filename}")]
	pub fn new(
		filename: &str,
		pyramid: Option<TileBBoxPyramid>,
		tile_size: u32,
		color: Option<Vec<u8>>,
	) -> Result<Self> {
		let parts = filename.split('.').collect::<Vec<_>>();
		ensure!(parts.len() == 2, "filename must have an extension");
		ensure!(parts[0].len() <= 4, "filename must be at most 4 characters long");

		let tile_format = match parts[1] {
			"avif" => TileFormat::AVIF,
			"png" => TileFormat::PNG,
			"jpg" | "jpeg" => TileFormat::JPG,
			"webp" => TileFormat::WEBP,
			_ => bail!("unknown file extension '{}'", parts[1]),
		};

		let pixel: Vec<u8> = if let Some(color) = color {
			color
		} else {
			parts[0]
				.iter_elements()
				.map(|c| {
					u8::from_str_radix(&c.to_string(), 16)
						.map(|v| v * 17)
						.map_err(anyhow::Error::from)
				})
				.collect::<Result<Vec<u8>>>()
				.with_context(|| format!("trying to parse filename '{}' as pixel value", parts[0]))?
		};
		let raw = Vec::from_iter(std::iter::repeat_n(pixel, (tile_size * tile_size) as usize).flatten());

		let image = DynamicImage::from_raw(tile_size as usize, tile_size as usize, raw)?;

		// Initialize the parameters with the given bounding box or a default one
		let parameters = TilesReaderParameters::new(
			tile_format,
			TileCompression::Uncompressed,
			pyramid.unwrap_or_else(|| TileBBoxPyramid::new_full(8)),
		);

		let mut tilejson = TileJSON::default();
		tilejson.set_string("name", "dummy raster source").unwrap();
		tilejson.update_from_reader_parameters(&parameters);

		let tile = Tile::from_image(image, tile_format)?;

		Ok(DummyImageSource {
			tile,
			parameters,
			tilejson,
		})
	}
}

#[async_trait]
impl TilesReaderTrait for DummyImageSource {
	fn source_name(&self) -> &str {
		"DummyImageSource"
	}

	fn container_name(&self) -> &str {
		"DummyImageSource"
	}

	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn override_compression(&mut self, _tile_compression: TileCompression) {
		panic!("not possible")
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		if !self.parameters.bbox_pyramid.contains_coord(coord) {
			return Ok(None);
		}
		Ok(Some(self.tile.clone()))
	}
}

#[async_trait]
impl OperationTrait for DummyImageSource {
	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}
	async fn get_stream(&self, mut bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_stream {:?}", bbox);
		let tile = self.tile.clone();
		bbox.intersect_with_pyramid(&self.parameters.bbox_pyramid);
		Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |_| {
			Some(tile.clone())
		}))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::GeoBBox;

	#[test]
	fn test_dummy_image_source_creation_valid_filename() {
		assert!(DummyImageSource::new("abcd.png", None, 4, None).is_ok());
	}

	#[test]
	fn test_dummy_image_source_creation_invalid_filename_extension() {
		assert!(DummyImageSource::new("abcd.xyz", None, 4, None).is_err());
	}

	#[test]
	fn test_dummy_image_source_creation_invalid_filename_length() {
		assert!(DummyImageSource::new("abcdef.png", None, 4, None).is_err());
	}

	#[tokio::test]
	async fn test_dummy_image_source_get_tile() {
		let source = DummyImageSource::new(
			"abcd.png",
			Some(TileBBoxPyramid::from_geo_bbox(
				0,
				8,
				&GeoBBox::new(-180.0, -90.0, 0.0, 0.0).unwrap(),
			)),
			4,
			None,
		)
		.unwrap();
		let tile_data = source.get_tile(&TileCoord::new(8, 0, 255).unwrap()).await.unwrap();
		assert!(tile_data.is_some());

		let tile_data = source.get_tile(&TileCoord::new(8, 0, 0).unwrap()).await.unwrap();
		assert!(tile_data.is_none());
	}

	#[tokio::test]
	async fn test_dummy_image_source_tilejson() {
		let source = DummyImageSource::new(
			"abcd.png",
			Some(TileBBoxPyramid::from_geo_bbox(
				3,
				15,
				&GeoBBox::new(-180.0, -90.0, 0.0, 0.0).unwrap(),
			)),
			4,
			None,
		)
		.unwrap();
		assert_eq!(
			OperationTrait::tilejson(&source).as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [-180, -85.051129, 0, 0],",
				"  \"maxzoom\": 15,",
				"  \"minzoom\": 3,",
				"  \"name\": \"dummy raster source\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tile_type\": \"raster\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);
	}
}
