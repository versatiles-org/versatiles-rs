use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use imageproc::image::DynamicImage;
use nom::Input;
use versatiles_core::{tilejson::TileJSON, types::*};
use versatiles_derive::context;
use versatiles_image::EnhancedDynamicImageTrait;

#[derive(Debug)]
pub struct MockImageSource {
	#[allow(clippy::type_complexity)]
	image: DynamicImage,
	parameters: TilesReaderParameters,
	tilejson: TileJSON,
}

impl MockImageSource {
	#[allow(clippy::type_complexity)]
	#[context("Creating MockImageSource for {filename}")]
	pub fn new(filename: &str, bbox: Option<TileBBoxPyramid>) -> Result<Self> {
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
		let raw = Vec::from_iter(std::iter::repeat_n(pixel, 16).flatten());

		let image = DynamicImage::from_raw(4, 4, raw)?;

		// Initialize the parameters with the given bounding box or a default one
		let parameters = TilesReaderParameters::new(
			tile_format,
			TileCompression::Uncompressed,
			bbox.unwrap_or_else(|| TileBBoxPyramid::new_full(8)),
		);

		let mut tilejson = TileJSON::default();
		tilejson.set_string("name", "mock raster source").unwrap();
		tilejson.update_from_reader_parameters(&parameters);

		Ok(MockImageSource {
			image,
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

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		if !self.parameters.bbox_pyramid.contains_coord(coord) {
			return Ok(None);
		}
		Ok(Some(self.image.to_blob(self.parameters.tile_format)?))
	}
}

#[cfg(test)]
pub fn arrange_tiles(tiles: Vec<(TileCoord3, Blob)>, cb: impl Fn(Blob) -> String) -> Vec<String> {
	use versatiles_core::types::TileBBox;

	let mut bbox = TileBBox::new_empty(tiles.first().unwrap().0.z).unwrap();
	tiles.iter().for_each(|t| bbox.include_coord(t.0.x, t.0.y));

	let mut result: Vec<Vec<String>> = (0..bbox.height())
		.map(|_| (0..bbox.width()).map(|_| String::from("❌")).collect())
		.collect();

	for (coord, blob) in tiles.into_iter() {
		let x = (coord.x - bbox.x_min) as usize;
		let y = (coord.y - bbox.y_min) as usize;
		result[y][x] = cb(blob);
	}
	result.into_iter().map(|r| r.join(" ")).collect::<Vec<String>>()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_mock_image_source_creation_valid_filename() {
		assert!(MockImageSource::new("abcd.png", None).is_ok());
	}

	#[test]
	fn test_mock_image_source_creation_invalid_filename_extension() {
		assert!(MockImageSource::new("abcd.xyz", None).is_err());
	}

	#[test]
	fn test_mock_image_source_creation_invalid_filename_length() {
		assert!(MockImageSource::new("abcdef.png", None).is_err());
	}

	#[tokio::test]
	async fn test_mock_image_source_get_tile_data() {
		let source = MockImageSource::new(
			"abcd.png",
			Some(TileBBoxPyramid::from_geo_bbox(0, 8, &GeoBBox(-180.0, -90.0, 0.0, 0.0))),
		)
		.unwrap();
		let tile_data = source
			.get_tile_data(&TileCoord3::new(0, 255, 8).unwrap())
			.await
			.unwrap();
		assert!(tile_data.is_some());

		let tile_data = source.get_tile_data(&TileCoord3::new(0, 0, 8).unwrap()).await.unwrap();
		assert!(tile_data.is_none());
	}

	#[tokio::test]
	async fn test_mock_image_source_tilejson() {
		let source = MockImageSource::new(
			"abcd.png",
			Some(TileBBoxPyramid::from_geo_bbox(3, 15, &GeoBBox(-180.0, -90.0, 0.0, 0.0))),
		)
		.unwrap();
		assert_eq!(
			source.tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 0, 0 ],",
				"  \"maxzoom\": 15,",
				"  \"minzoom\": 3,",
				"  \"name\": \"mock raster source\",",
				"  \"tile_content\": \"raster\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);
	}

	#[test]
	fn test_arrange_tiles() {
		let tiles = vec![
			(TileCoord3::new(0, 0, 1).unwrap(), Blob::from("a")),
			(TileCoord3::new(1, 0, 1).unwrap(), Blob::from("b")),
			(TileCoord3::new(0, 1, 1).unwrap(), Blob::from("c")),
		];
		let arranged = arrange_tiles(tiles, |blob| blob.as_str().to_string());
		assert_eq!(arranged, ["a b", "c ❌"]);
	}
}
