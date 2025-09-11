use crate::OperationTrait;
use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use imageproc::image::DynamicImage;
use nom::Input;
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::traits::*;

#[derive(Debug)]
pub struct MockImageSource {
	#[allow(clippy::type_complexity)]
	image: DynamicImage,
	blob: Blob,
	parameters: TilesReaderParameters,
	tilejson: TileJSON,
}

impl MockImageSource {
	#[allow(clippy::type_complexity)]
	#[context("Creating MockImageSource for {filename}")]
	pub fn new(filename: &str, pyramid: Option<TileBBoxPyramid>, tile_size: u32) -> Result<Self> {
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

		let pixel: Result<Vec<u8>> = parts[0]
			.iter_elements()
			.map(|c| {
				u8::from_str_radix(&c.to_string(), 16)
					.map(|v| v * 17)
					.map_err(anyhow::Error::from)
			})
			.collect();
		let pixel = pixel.with_context(|| format!("trying to parse filename '{}' as pixel value", parts[0]))?;
		let raw = Vec::from_iter(std::iter::repeat_n(pixel, (tile_size * tile_size) as usize).flatten());

		let image = DynamicImage::from_raw(tile_size, tile_size, raw)?;

		// Initialize the parameters with the given bounding box or a default one
		let parameters = TilesReaderParameters::new(
			tile_format,
			TileCompression::Uncompressed,
			pyramid.unwrap_or_else(|| TileBBoxPyramid::new_full(8)),
		);

		let mut tilejson = TileJSON::default();
		tilejson.set_string("name", "mock raster source").unwrap();
		tilejson.update_from_reader_parameters(&parameters);

		let blob = image.to_blob(tile_format)?;

		Ok(MockImageSource {
			image,
			blob,
			parameters,
			tilejson,
		})
	}
}

#[async_trait]
impl TilesReaderTrait for MockImageSource {
	fn source_name(&self) -> &str {
		"MockImageSource"
	}

	fn container_name(&self) -> &str {
		"MockImageSource"
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

	async fn get_tile_data(&self, coord: &TileCoord) -> Result<Option<Blob>> {
		if !self.parameters.bbox_pyramid.contains_coord(coord) {
			return Ok(None);
		}
		Ok(Some(self.blob.clone()))
	}
}

#[async_trait]
impl OperationTrait for MockImageSource {
	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}
	async fn get_tile_stream(&self, mut bbox: TileBBox) -> Result<TileStream<Blob>> {
		let blob = self.blob.clone();
		bbox.intersect_pyramid(&self.parameters.bbox_pyramid);
		Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |_| {
			Some(blob.clone())
		}))
	}

	async fn get_image_stream(&self, mut bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		let image = self.image.clone();
		bbox.intersect_pyramid(&self.parameters.bbox_pyramid);
		Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |_| {
			Some(image.clone())
		}))
	}

	async fn get_vector_stream(&self, _bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		bail!("Vector tiles are not supported in MockImageSource operations.");
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helpers::mock_vector_source::arrange_tiles;
	use versatiles_core::GeoBBox;

	#[test]
	fn test_mock_image_source_creation_valid_filename() {
		assert!(MockImageSource::new("abcd.png", None, 4).is_ok());
	}

	#[test]
	fn test_mock_image_source_creation_invalid_filename_extension() {
		assert!(MockImageSource::new("abcd.xyz", None, 4).is_err());
	}

	#[test]
	fn test_mock_image_source_creation_invalid_filename_length() {
		assert!(MockImageSource::new("abcdef.png", None, 4).is_err());
	}

	#[tokio::test]
	async fn test_mock_image_source_get_tile_data() {
		let source = MockImageSource::new(
			"abcd.png",
			Some(TileBBoxPyramid::from_geo_bbox(0, 8, &GeoBBox(-180.0, -90.0, 0.0, 0.0))),
			4,
		)
		.unwrap();
		let tile_data = source.get_tile_data(&TileCoord::new(8, 0, 255).unwrap()).await.unwrap();
		assert!(tile_data.is_some());

		let tile_data = source.get_tile_data(&TileCoord::new(8, 0, 0).unwrap()).await.unwrap();
		assert!(tile_data.is_none());
	}

	#[tokio::test]
	async fn test_mock_image_source_tilejson() {
		let source = MockImageSource::new(
			"abcd.png",
			Some(TileBBoxPyramid::from_geo_bbox(3, 15, &GeoBBox(-180.0, -90.0, 0.0, 0.0))),
			4,
		)
		.unwrap();
		assert_eq!(
			OperationTrait::tilejson(&source).as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 0, 0 ],",
				"  \"maxzoom\": 15,",
				"  \"minzoom\": 3,",
				"  \"name\": \"mock raster source\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tile_type\": \"raster\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);
	}

	#[test]
	fn test_arrange_tiles() {
		let tiles = vec![
			(TileCoord::new(1, 0, 0).unwrap(), Blob::from("a")),
			(TileCoord::new(1, 1, 0).unwrap(), Blob::from("b")),
			(TileCoord::new(1, 0, 1).unwrap(), Blob::from("c")),
		];
		let arranged = arrange_tiles(tiles, |blob| blob.as_str().to_string());
		assert_eq!(arranged, ["a b", "c ❌"]);
	}
}
