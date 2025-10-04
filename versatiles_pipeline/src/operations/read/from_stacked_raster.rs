//! # from_stacked_raster operation
//!
//! Combines *multiple* raster tile sources by **alpha‑blending** the tiles for
//! each coordinate.  
//!  
//! * Sources are evaluated **in the order given** – later sources overlay
//!   earlier ones.  
//! * Every source **must** produce raster tiles in the *same* resolution.  
//!
//! This file contains both the [`Args`] struct used by the VPL parser and the
//! [`Operation`] implementation that performs the blending.
use std::vec;

use crate::{
	PipelineFactory,
	helpers::Tile,
	operations::read::traits::ReadOperationTrait,
	traits::*,
	vpl::{VPLNode, VPLPipeline},
};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use futures::{
	StreamExt,
	future::{BoxFuture, join_all},
	stream,
};
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_image::traits::*;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Overlays multiple raster tile sources on top of each other.
struct Args {
	/// All tile sources must provide raster tiles in the same resolution.
	/// The first source overlays the others.
	sources: Vec<VPLPipeline>,

	/// The tile format to use for the output tiles.
	/// Default: format of the first source.
	format: Option<TileFormat>,
}

/// [`OperationTrait`] implementation that overlays raster tiles “on the fly.”
///
/// * Caches only metadata (`TileJSON`, `TilesReaderParameters`).  
/// * Performs no disk I/O itself; all data come from the child pipelines.
#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	sources: Vec<Box<dyn OperationTrait>>,
	tilejson: TileJSON,
	traversal: Traversal,
}

/// Blend a list of equally‑sized tiles using *source‑over* compositing.
/// First tile is in the front
///
/// Returns `Ok(None)` when the input list is empty.
fn stack_tiles(tiles: Vec<Tile>) -> Result<Option<Tile>> {
	let mut tile = Option::<Tile>::None;

	for mut tile_bg in tiles.into_iter() {
		if tile_bg.image()?.is_empty() {
			continue;
		}
		if let Some(mut image_fg) = tile {
			tile_bg.image_mut()?.overlay(image_fg.image()?)?;
		};
		tile = Some(tile_bg);
		if tile.as_mut().unwrap().image()?.is_opaque() {
			break;
		}
	}

	Ok(tile)
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

			ensure!(!sources.is_empty(), "must have at least one source");

			let mut tilejson = TileJSON::default();

			let first_parameters = sources.first().unwrap().parameters();
			let tile_format = args.format.unwrap_or(first_parameters.tile_format);
			ensure!(
				tile_format.get_type() == TileType::Raster,
				"output format must be a raster format"
			);
			let tile_compression = first_parameters.tile_compression;

			let mut pyramid = TileBBoxPyramid::new_empty();
			let mut traversal = Traversal::new_any();

			for source in sources.iter() {
				tilejson.merge(source.tilejson())?;

				traversal.intersect(source.traversal())?;

				let parameters = source.parameters();
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
				traversal,
			}) as Box<dyn OperationTrait>)
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	/// Reader parameters (format, compression, pyramid) for the *blended* result.
	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	/// Combined `TileJSON` derived from all sources.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn traversal(&self) -> &Traversal {
		&self.traversal
	}

	/// Stream packed raster tiles intersecting `bbox`.
	async fn get_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_stream {:?}", bbox);

		let bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(16).collect();
		let sources = &self.sources;
		let tile_format = self.parameters.tile_format;
		let tile_compression = self.parameters.tile_compression;

		Ok(TileStream::from_streams(stream::iter(bboxes).map(
			move |bbox| async move {
				let mut tiles = TileBBoxMap::<Vec<Tile>>::new_default(bbox);

				let streams = sources.iter().map(|source| source.get_stream(bbox));
				let results = futures::future::join_all(streams).await;

				for result in results.into_iter() {
					let result = result.unwrap();
					result
						.for_each_sync(|(coord, mut tile)| {
							let image = tile.image().unwrap();
							if !image.is_empty() {
								tiles.get_mut(&coord).unwrap().push(tile);
							}
						})
						.await;
				}

				let v = tiles
					.into_iter()
					.filter_map(|(c, v)| match stack_tiles(v) {
						Ok(Some(mut tile)) => Some((|| {
							tile.change_compression(tile_compression)?;
							tile.change_format(tile_format)?;
							Ok((c, tile))
						})()),
						Ok(None) => None,
						Err(err) => Some(Err(err)),
					})
					.collect::<Result<Vec<_>>>()
					.unwrap();

				TileStream::from_vec(v)
			},
		)))
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"from_stacked_raster"
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
	use super::*;
	use crate::helpers::{arrange_tiles, dummy_image_source::DummyImageSource};
	use imageproc::image::GenericImage;
	use versatiles_image::DynamicImage;

	pub fn get_color(blob: &Blob) -> String {
		let image = DynamicImage::from_blob(blob, TileFormat::PNG).unwrap();
		let pixel = image.iter_pixels().next().unwrap();
		pixel.iter().map(|v| format!("{v:02X}")).collect::<Vec<_>>().join("")
	}

	#[tokio::test]
	async fn test_operation_error() {
		let factory = PipelineFactory::new_dummy();
		let error = |command: &'static str| async {
			assert_eq!(
				factory.operation_from_vpl(command).await.unwrap_err().to_string(),
				"must have at least one source"
			)
		};

		error("from_stacked_raster").await;
		error("from_stacked_raster [ ]").await;
	}

	#[tokio::test]
	async fn test_tilejson() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("from_stacked_raster [ from_container filename=07.png, from_container filename=F7.png ]")
			.await?;

		assert_eq!(
			result.tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"dummy raster source\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tile_type\": \"raster\",",
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
				r#"from_stacked_raster [
					from_container filename="00F7.png" | filter bbox=[-130,-20,20,70],
					from_container filename="FF07.png" | filter bbox=[-20,-70,130,20]
				]"#,
			)
			.await?;

		let bbox = TileBBox::new_full(3)?;
		let tiles = result.get_stream(bbox).await?.to_vec().await;

		assert_eq!(
			arrange_tiles(tiles, |mut tile| {
				match get_color(tile.blob().unwrap()).as_str() {
					"0000FF77" => "🟦",
					"FFFF0077" => "🟨",
					"5858A6B6" => "🟩",
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
			result.tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -130.78125, -70.140364, 130.78125, 70.140364 ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"dummy raster source\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tile_type\": \"raster\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_operation_parameters() -> Result<()> {
		let factory = PipelineFactory::new_dummy_reader(Box::new(
			|filename: String| -> BoxFuture<Result<Box<dyn TilesReaderTrait>>> {
				Box::pin(async move {
					let mut pyramide = TileBBoxPyramid::new_empty();
					for c in filename[0..filename.len() - 4].chars() {
						pyramide.include_bbox(&TileBBox::new_full(c.to_digit(10).unwrap() as u8)?);
					}
					Ok(Box::new(DummyImageSource::new(&filename, Some(pyramide), 4).unwrap()) as Box<dyn TilesReaderTrait>)
				})
			},
		));

		let result = factory
			.operation_from_vpl(
				r#"from_stacked_raster [ from_container filename="12.png", from_container filename="23.png" ]"#,
			)
			.await?;

		let parameters = result.parameters();

		assert_eq!(parameters.tile_format, TileFormat::PNG);
		assert_eq!(parameters.tile_compression, TileCompression::Uncompressed);
		assert_eq!(
			format!("{}", parameters.bbox_pyramid),
			"[1: [0,0,1,1] (2x2), 2: [0,0,3,3] (4x4), 3: [0,0,7,7] (8x8)]"
		);

		assert_eq!(
			result.tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 3,",
				"  \"minzoom\": 1,",
				"  \"name\": \"dummy raster source\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tile_type\": \"raster\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_merge_tiles_multiple_layers() -> Result<()> {
		use versatiles_core::{TileCompression::Uncompressed, TileFormat::PNG};
		let tile1 = Tile::from_image(DynamicImage::new_test_rgb(), PNG, Uncompressed);
		let tile2 = Tile::from_image(DynamicImage::new_test_rgba(), PNG, Uncompressed);

		let _merged_tile = stack_tiles(vec![tile1, tile2])?.unwrap();

		Ok(())
	}

	#[tokio::test]
	async fn test_reuses_original_blob_single_source() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let stacked = factory
			.operation_from_vpl("from_stacked_raster [ from_container filename=00F7.png ]")
			.await?;
		// Build the plain source to compare raw tiles
		let plain = factory.operation_from_vpl("from_container filename=00F7.png").await?;

		let bbox = TileBBox::new_full(3)?;
		let stacked_tiles = stacked.get_stream(bbox).await?.to_vec().await;
		let plain_tiles = plain.get_stream(bbox).await?.to_vec().await;

		// Convert to maps for easy lookup
		use std::collections::HashMap;
		let mut map_stacked: HashMap<_, _> = stacked_tiles.into_iter().collect();
		let map_plain: HashMap<_, _> = plain_tiles.into_iter().collect();

		// For every key present in the plain source, the stacked version must be byte-identical
		for (coord, mut tile_plain) in map_plain.into_iter() {
			if let Some(mut tile_stacked) = map_stacked.remove(&coord) {
				assert_eq!(
					tile_stacked.blob().unwrap().as_slice(),
					tile_plain.blob().unwrap().as_slice()
				);
			}
		}
		Ok(())
	}

	#[tokio::test]
	async fn test_reencodes_on_blend() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let stacked = factory
			.operation_from_vpl(
				"from_stacked_raster [ from_container filename=00F7.png, from_container filename=FF07.png ]",
			)
			.await?;
		let src1 = factory.operation_from_vpl("from_container filename=00F7.png").await?;
		let src2 = factory.operation_from_vpl("from_container filename=FF07.png").await?;

		let bbox = TileBBox::new_full(3)?;
		let coord = TileCoord::new(3, 2, 2)?; // a tile that lies in the overlap area in our dummy dataset

		let stacked_tile = stacked.get_stream(bbox).await?.to_map().await.remove(&coord);
		let tile1 = src1.get_stream(bbox).await?.to_map().await.remove(&coord);
		let tile2 = src2.get_stream(bbox).await?.to_map().await.remove(&coord);

		if let Some(mut stacked_tile) = stacked_tile {
			// If both sources produced a tile here, blended output must differ from each single-source blob
			if let (Some(mut tile1), Some(mut tile2)) = (tile1, tile2) {
				assert_ne!(
					stacked_tile.blob().unwrap().as_slice(),
					tile1.blob().unwrap().as_slice()
				);
				assert_ne!(
					stacked_tile.blob().unwrap().as_slice(),
					tile2.blob().unwrap().as_slice()
				);
			}
		}
		Ok(())
	}

	#[test]
	fn stack_tiles_empty_returns_none() {
		let out = stack_tiles(Vec::new()).unwrap();
		assert!(out.is_none());
	}

	#[test]
	fn stack_tiles_opaque_first_short_circuits() -> Result<()> {
		use versatiles_core::{TileCompression::Uncompressed, TileFormat::PNG};

		// First tile: fully opaque red 2x2
		let mut a = DynamicImage::new_rgba8(2, 2);
		for y in 0..2 {
			for x in 0..2 {
				a.put_pixel(x, y, imageproc::image::Rgba([255, 0, 0, 255]));
			}
		}
		let mut a = Tile::from_image(a, PNG, Uncompressed);

		// Second tile: green; would change pixels if blended, but should be ignored due to early break
		let mut b = DynamicImage::new_rgba8(2, 2);
		for y in 0..2 {
			for x in 0..2 {
				b.put_pixel(x, y, imageproc::image::Rgba([0, 255, 0, 255]));
			}
		}
		let b = Tile::from_image(b, PNG, Uncompressed);

		let mut res = stack_tiles(vec![a.clone(), b])?.unwrap();
		assert_eq!(res.blob()?, a.blob()?);

		Ok(())
	}
}
