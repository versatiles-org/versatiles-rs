use crate::{
	helpers::{pack_image_tile, pack_image_tile_stream},
	operations::read::traits::ReadOperationTrait,
	traits::*,
	vpl::{VPLNode, VPLPipeline},
	PipelineFactory,
};
use anyhow::{bail, ensure, Result};
use async_trait::async_trait;
use futures::future::{join_all, BoxFuture};
use imageproc::image::DynamicImage;
use versatiles_core::{tilejson::TileJSON, types::*};
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::EnhancedDynamicImageTrait;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Merges multiple vector tile sources. Each layer will contain all features from the same layer of all sources.
struct Args {
	/// All tile sources must provide vector tiles.
	sources: Vec<VPLPipeline>,
	/// The tile format to use for the output tiles (default: PNG).
	format: Option<TileFormat>,
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	sources: Vec<Box<dyn OperationTrait>>,
	tilejson: TileJSON,
}

fn overlay_image_tiles(tiles: Vec<DynamicImage>) -> Result<Option<DynamicImage>> {
	let mut image = Option::<DynamicImage>::None;
	for tile in tiles.into_iter() {
		image = Some(if image.is_none() {
			tile
		} else {
			let mut image = image.unwrap();
			image.overlay(&tile)?;
			image
		});
	}
	Ok(image)
}

impl ReadOperationTrait for Operation {
	fn build(
		vpl_node: VPLNode,
		factory: &PipelineFactory,
	) -> BoxFuture<'_, Result<Box<dyn OperationTrait>, anyhow::Error>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			let sources = join_all(args.sources.into_iter().map(|c| factory.build_pipeline(c)))
				.await
				.into_iter()
				.collect::<Result<Vec<_>>>()?;

			ensure!(sources.len() > 1, "must have at least two sources");

			let mut tilejson = TileJSON::default();
			let first_parameters = sources.first().unwrap().get_parameters();
			let tile_format = args.format.unwrap_or(TileFormat::PNG);
			let tile_compression = first_parameters.tile_compression;
			let mut pyramid = TileBBoxPyramid::new_empty();

			for source in sources.iter() {
				tilejson.merge(source.get_tilejson())?;

				let parameters = source.get_parameters();
				pyramid.include_bbox_pyramid(&parameters.bbox_pyramid);

				ensure!(
					parameters.tile_format.get_type() == TileType::Raster,
					"all sources must be raster tiles"
				);
			}

			let parameters = TilesReaderParameters::new(tile_format, tile_compression, pyramid);
			tilejson.update_from_reader_parameters(&parameters);

			Ok(Box::new(Self {
				tilejson,
				parameters,
				sources,
			}) as Box<dyn OperationTrait>)
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn get_tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		pack_image_tile(self.get_image_data(coord).await, &self.parameters)
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		pack_image_tile_stream(self.get_image_stream(bbox).await, &self.parameters)
	}

	async fn get_vector_data(&self, _coord: &TileCoord3) -> Result<Option<VectorTile>> {
		bail!("this operation does not support vector data");
	}

	async fn get_vector_stream(&self, _bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		bail!("this operation does not support vector data");
	}

	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		let mut images: Vec<DynamicImage> = vec![];
		for source in self.sources.iter() {
			let image = source.get_image_data(coord).await?;
			if let Some(image) = image {
				images.push(image);
			}
		}

		if images.is_empty() {
			Ok(None)
		} else {
			overlay_image_tiles(images)
		}
	}

	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		let bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(32).collect();

		Ok(
			TileStream::from_stream_iter(bboxes.into_iter().map(move |bbox| async move {
				let mut images: Vec<Vec<DynamicImage>> = Vec::new();
				images.resize(bbox.count_tiles() as usize, vec![]);

				for source in self.sources.iter() {
					source
						.get_image_stream(bbox)
						.await
						.unwrap()
						.for_each_sync(|(coord, tile)| {
							images[bbox.get_tile_index3(&coord).unwrap()].push(tile);
						})
						.await;
				}

				TileStream::from_vec(
					images
						.into_iter()
						.enumerate()
						.filter_map(|(i, v)| {
							if v.is_empty() {
								None
							} else {
								Some((
									bbox.get_coord3_by_index(i as u32).unwrap(),
									overlay_image_tiles(v).unwrap().unwrap(),
								))
							}
						})
						.collect(),
				)
			}))
			.await,
		)
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"merge_imagetiles"
	}
}

#[async_trait]
impl ReadOperationFactoryTrait for Factory {
	async fn build<'a>(&self, vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn OperationTrait>> {
		Operation::build(vpl_node, factory).await
	}
}

#[cfg(test)]
mod tests {
	use crate::helpers::mock_image_source::{arrange_tiles, MockImageSource};

	use super::*;
	use std::{ops::BitXor, path::Path};

	pub fn get_color(blob: &Blob) -> String {
		let image = DynamicImage::from_blob(blob, TileFormat::PNG).unwrap();
		let pixel = image.pixels().next().unwrap();
		pixel.iter().map(|v| format!("{v:02X}")).collect::<Vec<_>>().join("")
	}

	#[tokio::test]
	async fn test_operation_error() {
		let factory = PipelineFactory::new_dummy();
		let error = |command: &'static str| async {
			assert_eq!(
				factory.operation_from_vpl(command).await.unwrap_err().to_string(),
				"must have at least two sources"
			)
		};

		error("merge_imagetiles").await;
		error("merge_imagetiles [ ]").await;
		error("merge_imagetiles [ from_container filename=1.png ]").await;
	}

	#[tokio::test]
	async fn test_operation_get_tile_data() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("merge_imagetiles [ from_container filename=07.png, from_container filename=F7.png ]")
			.await?;

		let coord = TileCoord3::new(1, 2, 3)?;
		let blob = result.get_tile_data(&coord).await?.unwrap();

		assert_eq!(get_color(&blob), "A6B6");
		assert_eq!(
			result.get_tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"mock raster source\",",
				"  \"tile_content\": \"raster\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_operation_get_tile_stream() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				r#"merge_imagetiles [
					from_container filename="00F7.png" | filter_bbox bbox=[-130,-20,20,70],
					from_container filename="FF07.png" | filter_bbox bbox=[-20,-70,130,20]
				]"#,
			)
			.await?;

		let bbox = TileBBox::new_full(3)?;
		let tiles = result.get_tile_stream(bbox).await?.collect().await;

		assert_eq!(
			arrange_tiles(tiles, |blob| {
				match get_color(&blob).as_str() {
					"0000FF77" => "🟦",
					"FFFF0077" => "🟨",
					"A6A658B6" => "🟩",
					e => panic!("{}", e),
				}
				.to_string()
			}),
			vec![
				"🟦 🟦 🟦 🟦 ❌ ❌",
				"🟦 🟦 🟦 🟦 ❌ ❌",
				"🟦 🟦 🟩 🟩 🟨 🟨",
				"🟦 🟦 🟩 🟩 🟨 🟨",
				"❌ ❌ 🟨 🟨 🟨 🟨",
				"❌ ❌ 🟨 🟨 🟨 🟨"
			]
		);

		assert_eq!(
			result.get_tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -130.78125, -70.140364, 130.78125, 70.140364 ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"mock raster source\",",
				"  \"tile_content\": \"raster\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_operation_parameters() -> Result<()> {
		let factory = PipelineFactory::default(
			Path::new(""),
			Box::new(|filename: String| -> BoxFuture<Result<Box<dyn TilesReaderTrait>>> {
				Box::pin(async move {
					let mut pyramide = TileBBoxPyramid::new_empty();
					for c in filename[0..filename.len() - 4].chars() {
						pyramide.include_bbox(&TileBBox::new_full(c.to_digit(10).unwrap() as u8)?);
					}
					Ok(Box::new(MockImageSource::new(&filename, Some(pyramide)).unwrap()) as Box<dyn TilesReaderTrait>)
				})
			}),
		);

		let result = factory
			.operation_from_vpl(
				r#"merge_imagetiles [ from_container filename="12.png", from_container filename="23.png" ]"#,
			)
			.await?;

		let parameters = result.get_parameters();

		assert_eq!(parameters.tile_format, TileFormat::PNG);
		assert_eq!(parameters.tile_compression, TileCompression::Uncompressed);
		assert_eq!(
			format!("{}", parameters.bbox_pyramid),
			"[1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64)]"
		);

		for level in 0..=4 {
			assert!(
				result
					.get_tile_data(&TileCoord3::new(0, 0, level)?)
					.await?
					.is_some()
					.bitxor(!(1..=3).contains(&level)),
				"level: {level}"
			);
		}

		assert_eq!(
			result.get_tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 3,",
				"  \"minzoom\": 1,",
				"  \"name\": \"mock raster source\",",
				"  \"tile_content\": \"raster\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_merge_tiles_multiple_layers() -> Result<()> {
		let image1 = DynamicImage::new_test_rgb();
		let image2 = DynamicImage::new_test_rgba();

		let _merged_tile = overlay_image_tiles(vec![image1, image2])?.unwrap();

		Ok(())
	}
}
