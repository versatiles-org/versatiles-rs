use anyhow::{bail, ensure, Result};
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

		let pixel = parts[0]
			.iter_elements()
			.map(|c| u8::from_str_radix(&c.to_string(), 16).unwrap() * 17)
			.collect::<Vec<_>>();
		let raw = Vec::from_iter(std::iter::repeat_n(pixel, 16).flatten());

		let image = DynamicImage::from_raw(4, 4, raw)?;

		// Initialize the parameters with the given bounding box or a default one
		let parameters = TilesReaderParameters::new(
			tile_format,
			TileCompression::Uncompressed,
			bbox.unwrap_or_else(|| TileBBoxPyramid::new_full(8)),
		);

		let mut tilejson = TileJSON::default();
		tilejson.set_string("type", "mock vector source").unwrap();

		Ok(MockImageSource {
			image,
			parameters,
			tilejson,
		})
	}
}

#[async_trait]
impl TilesReaderTrait for MockImageSource {
	fn get_source_name(&self) -> &str {
		"MockImageSource"
	}

	fn get_container_name(&self) -> &str {
		"MockImageSource"
	}

	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn override_compression(&mut self, _tile_compression: TileCompression) {
		panic!("not possible")
	}

	fn get_tilejson(&self) -> &TileJSON {
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
