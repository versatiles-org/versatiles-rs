use crate::OperationTrait;
use anyhow::{Result, ensure};
use async_trait::async_trait;
use imageproc::image::DynamicImage;
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
	#[context("Creating DummyImageSource, tile_format='{tile_format}', tile_size={tile_size}")]
	pub fn new(tile_format: TileFormat, color: &[u8], tile_size: u32, pyramid: Option<TileBBoxPyramid>) -> Result<Self> {
		ensure!(tile_format.is_raster(), "tile_format must be a raster format");
		ensure!(!color.is_empty(), "color vector must not be empty");
		ensure!(color.len() <= 4, "color vector length must be between 1 and 4");
		ensure!(tile_size > 0, "tile_size must be greater than zero");

		let color: Vec<u8> = color.to_vec();
		let raw = Vec::from_iter(std::iter::repeat_n(color, (tile_size * tile_size) as usize).flatten());
		let image = DynamicImage::from_raw(tile_size as usize, tile_size as usize, raw)?;

		DummyImageSource::from_image(tile_format, image, pyramid)
	}

	#[context("Creating DummyImageSource from image, tile_format='{tile_format}'")]
	pub fn from_image(tile_format: TileFormat, image: DynamicImage, pyramid: Option<TileBBoxPyramid>) -> Result<Self> {
		ensure!(tile_format.is_raster(), "tile_format must be a raster format");

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
	use versatiles_core::{GeoBBox, TileFormat::*};

	#[test]
	fn test_dummy_image_source_creation_valid() {
		assert!(DummyImageSource::new(PNG, &[0, 100, 200], 4, None).is_ok());
	}

	#[test]
	fn test_dummy_image_source_creation_invalid_format() {
		assert!(DummyImageSource::new(SVG, &[0, 100, 200], 4, None).is_err());
	}

	#[test]
	fn test_dummy_image_source_creation_invalid_color() {
		assert!(DummyImageSource::new(PNG, &[], 4, None).is_err());
		assert!(DummyImageSource::new(PNG, &[1, 2, 3, 4, 5], 4, None).is_err());
	}

	#[tokio::test]
	async fn test_dummy_image_source_get_tile() {
		let source = DummyImageSource::new(
			PNG,
			&[0, 100, 200],
			4,
			Some(TileBBoxPyramid::from_geo_bbox(
				0,
				8,
				&GeoBBox::new(-180.0, -90.0, 0.0, 0.0).unwrap(),
			)),
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
			PNG,
			&[0, 100, 200],
			4,
			Some(TileBBoxPyramid::from_geo_bbox(
				3,
				15,
				&GeoBBox::new(-180.0, -90.0, 0.0, 0.0).unwrap(),
			)),
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
