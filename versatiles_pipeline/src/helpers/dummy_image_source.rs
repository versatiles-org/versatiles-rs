use crate::OperationTrait;
use anyhow::{Result, ensure};
use async_trait::async_trait;
use imageproc::image::DynamicImage;
use std::sync::Arc;
use versatiles_container::{Tile, TilesReaderTrait};
use versatiles_core::*;
use versatiles_derive::context;
use versatiles_image::traits::*;

pub struct DummyImageSource {
	#[allow(clippy::type_complexity)]
	generate_tile: Arc<dyn Fn(&TileCoord) -> Option<Tile> + Send + Sync>,
	parameters: TilesReaderParameters,
	tilejson: TileJSON,
}

impl DummyImageSource {
	#[context("Creating DummyImageSource, tile_format='{tile_format}', tile_size={tile_size}")]
	pub fn from_color(
		color: &[u8],
		tile_size: u32,
		tile_format: TileFormat,
		pyramid: Option<TileBBoxPyramid>,
	) -> Result<Self> {
		ensure!(tile_format.is_raster(), "tile_format must be a raster format");
		ensure!(!color.is_empty(), "color vector must not be empty");
		ensure!(color.len() <= 4, "color vector length must be between 1 and 4");
		ensure!(tile_size > 0, "tile_size must be greater than zero");

		let color: Vec<u8> = color.to_vec();
		let raw = Vec::from_iter(std::iter::repeat_n(color, (tile_size * tile_size) as usize).flatten());
		let image = DynamicImage::from_raw(tile_size as usize, tile_size as usize, raw)?;

		DummyImageSource::from_image(image, tile_format, pyramid)
	}

	#[context("Creating DummyImageSource from image, tile_format='{tile_format}'")]
	pub fn from_image(image: DynamicImage, tile_format: TileFormat, pyramid: Option<TileBBoxPyramid>) -> Result<Self> {
		ensure!(tile_format.is_raster(), "tile_format must be a raster format");
		let tile = Arc::new(Tile::from_image(image, tile_format)?);
		Self::new(move |_coord| Some((*tile).clone()), tile_format, pyramid)
	}

	#[context("Creating DummyImageSource from image, tile_format='{tile_format}'")]
	pub fn new<F>(generate_tile: F, tile_format: TileFormat, pyramid: Option<TileBBoxPyramid>) -> Result<Self>
	where
		F: Fn(&TileCoord) -> Option<Tile> + Send + Sync + 'static,
	{
		ensure!(tile_format.is_raster(), "tile_format must be a raster format");

		let parameters = TilesReaderParameters::new(
			tile_format,
			TileCompression::Uncompressed,
			pyramid.unwrap_or_else(|| TileBBoxPyramid::new_full(8)),
		);

		let mut tilejson = TileJSON::default();
		tilejson.set_string("name", "dummy raster source")?;
		tilejson.update_from_reader_parameters(&parameters);

		Ok(DummyImageSource {
			generate_tile: Arc::new(generate_tile),
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

	#[context("Getting tile for coord: {:?}", coord)]
	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		if !self.parameters.bbox_pyramid.contains_coord(coord) {
			return Ok(None);
		}
		Ok((self.generate_tile)(coord))
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

	#[context("Failed to get stream for bbox: {:?}", bbox)]
	async fn get_stream(&self, mut bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_stream {:?}", bbox);

		let generate_tile = (self.generate_tile).clone();
		bbox.intersect_with_pyramid(&self.parameters.bbox_pyramid);
		Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |coord| {
			(generate_tile)(&coord)
		}))
	}
}

impl std::fmt::Debug for DummyImageSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DummyImageSource")
			.field("tile_format", &self.parameters.tile_format)
			.field("tile_compression", &self.parameters.tile_compression)
			.field("bbox_pyramid", &self.parameters.bbox_pyramid)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::{GeoBBox, TileFormat::*};

	#[test]
	fn test_dummy_image_source_creation_valid() {
		assert!(DummyImageSource::from_color(&[0, 100, 200], 4, PNG, None).is_ok());
	}

	#[test]
	fn test_dummy_image_source_creation_invalid_format() {
		assert!(DummyImageSource::from_color(&[0, 100, 200], 4, SVG, None).is_err());
	}

	#[test]
	fn test_dummy_image_source_creation_invalid_color() {
		assert!(DummyImageSource::from_color(&[], 4, PNG, None).is_err());
		assert!(DummyImageSource::from_color(&[1, 2, 3, 4, 5], 4, PNG, None).is_err());
	}

	#[tokio::test]
	async fn test_dummy_image_source_get_tile() {
		let source = DummyImageSource::from_color(
			&[0, 100, 200],
			4,
			PNG,
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
		let source = DummyImageSource::from_color(
			&[0, 100, 200],
			4,
			PNG,
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
