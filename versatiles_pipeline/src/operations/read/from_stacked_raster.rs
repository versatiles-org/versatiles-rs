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
	helpers::pack_image_tile_stream,
	operations::read::traits::ReadOperationTrait,
	traits::*,
	vpl::{VPLNode, VPLPipeline},
};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use futures::{
	StreamExt,
	future::{BoxFuture, join_all},
	stream,
};
use imageproc::image::DynamicImage;
use versatiles_core::{
	tilejson::TileJSON,
	utils::{compress, decompress},
	*,
};
use versatiles_geometry::vector_tile::VectorTile;
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

	/// Try to avoid unnecessary image recompression: If the blended result is identical to one of
	/// the input tiles, the operation will reuse that tile’s original binary data instead of
	/// recompressing it. This prevents unnecessary compression artifacts.
	minimize_recompression: Option<bool>,
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
	minimize_recompression: bool,
}

/// Blend a list of equally‑sized tiles using *source‑over* compositing.
/// First tile is in the front
///
/// Returns `Ok(None)` when the input list is empty.
fn stack_images<T>(images: Vec<(DynamicImage, T)>) -> Result<Option<(DynamicImage, Option<T>)>> {
	let mut image = Option::<DynamicImage>::None;
	let mut indexes = vec![];

	for (mut image_bg, index) in images.into_iter() {
		if image_bg.is_empty() {
			continue;
		}
		indexes.push(index);
		if let Some(image_fg) = image {
			image_bg.overlay(&image_fg)?;
		};
		image = Some(image_bg);
		if image.as_ref().unwrap().is_opaque() {
			break;
		}
	}

	let index = if indexes.len() == 1 { indexes.pop() } else { None };
	Ok(image.map(|img| (img, index)))
}

async fn get_stacked_images(
	sources: &[Box<dyn OperationTrait>],
	bbox: TileBBox,
) -> Result<Vec<(TileCoord, DynamicImage)>> {
	let mut images = TileBBoxMap::<Vec<(DynamicImage, u8)>>::new_default(bbox);

	let streams = sources.iter().map(|source| source.get_image_stream(bbox));
	let results = futures::future::join_all(streams).await;

	for result in results.into_iter() {
		let result = result?;
		result
			.for_each_sync(|(coord, tile)| {
				if !tile.is_empty() {
					images.get_mut(&coord).unwrap().push((tile, 0));
				}
			})
			.await;
	}

	images
		.into_iter()
		.filter_map(|(c, v)| match stack_images(v) {
			Ok(Some((img, _))) => Some(Ok((c, img))),
			Ok(None) => None,
			Err(err) => Some(Err(err)),
		})
		.collect::<Result<Vec<_>>>()
}

async fn get_stacked_blobs(
	sources: &[Box<dyn OperationTrait>],
	bbox: TileBBox,
	tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<Vec<(TileCoord, Blob)>> {
	let mut images = TileBBoxMap::<Vec<(DynamicImage, Blob)>>::new_default(bbox);

	let streams = sources.iter().map(|source| async move {
		let stream = source.get_blob_stream(bbox).await.unwrap();
		let tile_format = source.parameters().tile_format;
		let tile_compression = source.parameters().tile_compression;
		stream.map_item_parallel(move |blob| {
			let blob = decompress(blob, &tile_compression)?;
			let image = DynamicImage::from_blob(&blob, tile_format).unwrap();
			Ok((image, blob))
		})
	});
	let results = futures::future::join_all(streams).await;

	for result in results.into_iter() {
		result
			.for_each_sync(|(coord, (image, blob))| {
				if !image.is_empty() {
					images.get_mut(&coord).unwrap().push((image, blob));
				}
			})
			.await;
	}

	images
		.into_iter()
		.filter_map(|(c, v)| match stack_images(v) {
			Ok(Some((_, Some(blob)))) => Some(Ok((c, compress(blob, &tile_compression).unwrap()))),
			Ok(Some((img, None))) => Some(Ok((
				c,
				compress(img.to_blob(tile_format).unwrap(), &tile_compression).unwrap(),
			))),
			Ok(None) => None,
			Err(err) => Some(Err(err)),
		})
		.collect::<Result<Vec<_>>>()
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

			let minimize_recompression = args.minimize_recompression.unwrap_or(false);
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
				minimize_recompression,
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
	async fn get_blob_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		log::debug!("get_blob_stream {:?}", bbox);

		if self.minimize_recompression {
			let bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(32).collect();
			let sources = &self.sources;
			let tile_format = self.parameters.tile_format;
			let tile_compression = self.parameters.tile_compression;

			Ok(TileStream::from_streams(stream::iter(bboxes).map(
				move |bbox| async move {
					TileStream::from_vec(
						get_stacked_blobs(sources, bbox, tile_format, tile_compression)
							.await
							.unwrap(),
					)
				},
			)))
		} else {
			pack_image_tile_stream(self.get_image_stream(bbox).await, &self.parameters)
		}
	}

	/// Always errors – vector output is not supported.
	async fn get_vector_stream(&self, _bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		bail!("this operation does not support vector data");
	}

	/// Stream blended raster tiles for every coordinate inside `bbox`.
	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		log::debug!("get_image_stream {:?}", bbox);

		let bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(32).collect();
		let sources = &self.sources;

		Ok(TileStream::from_streams(stream::iter(bboxes).map(
			move |bbox| async move { TileStream::from_vec(get_stacked_images(sources, bbox).await.unwrap()) },
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
	use crate::helpers::{dummy_image_source::DummyImageSource, dummy_vector_source::arrange_tiles};
	use imageproc::image::GenericImage;

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
		let tiles = result.get_blob_stream(bbox).await?.to_vec().await;

		assert_eq!(
			arrange_tiles(tiles, |blob| {
				match get_color(&blob).as_str() {
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
		let image1 = DynamicImage::new_test_rgb();
		let image2 = DynamicImage::new_test_rgba();

		let _merged_tile = stack_images(vec![(image1, 0), (image2, 1)])?.unwrap();

		Ok(())
	}

	#[tokio::test]
	async fn test_minimize_recompression_reuses_original_blob_single_source() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		// Build the stacked op with a single source and minimize_recompression=true
		let stacked = factory
			.operation_from_vpl("from_stacked_raster minimize_recompression=true [ from_container filename=00F7.png ]")
			.await?;
		// Build the plain source to compare raw tiles
		let plain = factory.operation_from_vpl("from_container filename=00F7.png").await?;

		let bbox = TileBBox::new_full(3)?;
		let stacked_tiles = stacked.get_blob_stream(bbox).await?.to_vec().await;
		let plain_tiles = plain.get_blob_stream(bbox).await?.to_vec().await;

		// Convert to maps for easy lookup
		use std::collections::HashMap;
		let map_stacked: HashMap<_, _> = stacked_tiles.into_iter().collect();
		let map_plain: HashMap<_, _> = plain_tiles.into_iter().collect();

		// For every key present in the plain source, the stacked version must be byte-identical
		for (coord, blob_plain) in map_plain.iter() {
			if let Some(blob_stacked) = map_stacked.get(coord) {
				assert_eq!(
					blob_stacked.as_slice(),
					blob_plain.as_slice(),
					"expected minimize_recompression to reuse original bytes for {coord:?}"
				);
			}
		}
		Ok(())
	}

	#[tokio::test]
	async fn test_minimize_recompression_reencodes_on_blend() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		// Two sources overlapping; minimize_recompression should NOT reuse original bytes when pixels change
		let stacked = factory
            .operation_from_vpl(
                "from_stacked_raster minimize_recompression=true [ from_container filename=00F7.png, from_container filename=FF07.png ]",
            )
            .await?;
		let src1 = factory.operation_from_vpl("from_container filename=00F7.png").await?;
		let src2 = factory.operation_from_vpl("from_container filename=FF07.png").await?;

		let bbox = TileBBox::new_full(3)?;
		let coord = TileCoord::new(3, 2, 2)?; // a tile that lies in the overlap area in our dummy dataset

		let stacked_blob = stacked.get_blob_stream(bbox).await?.to_map().await.remove(&coord);
		let blob1 = src1.get_blob_stream(bbox).await?.to_map().await.remove(&coord);
		let blob2 = src2.get_blob_stream(bbox).await?.to_map().await.remove(&coord);

		if let Some(b_stacked) = stacked_blob {
			// If both sources produced a tile here, blended output must differ from each single-source blob
			if let (Some(b1), Some(b2)) = (blob1, blob2) {
				assert_ne!(b_stacked.as_slice(), b1.as_slice());
				assert_ne!(b_stacked.as_slice(), b2.as_slice());
			}
		}
		Ok(())
	}

	#[test]
	fn stack_images_empty_returns_none() {
		let out: Option<(DynamicImage, Option<u8>)> = stack_images::<u8>(Vec::new()).unwrap();
		assert!(out.is_none());
	}

	#[test]
	fn stack_images_single_returns_image_and_index() {
		let img = DynamicImage::new_rgb8(2, 2);
		let out = stack_images(vec![(img, 7u8)]).unwrap();
		assert!(out.is_some());
		let (res, idx) = out.unwrap();
		assert_eq!(res.width(), 2);
		assert_eq!(res.height(), 2);
		assert_eq!(idx, Some(7));
	}

	#[test]
	fn stack_images_opaque_first_short_circuits() {
		// First tile: fully opaque red 2x2
		let mut a = DynamicImage::new_rgba8(2, 2);
		for y in 0..2 {
			for x in 0..2 {
				a.put_pixel(x, y, imageproc::image::Rgba([255, 0, 0, 255]));
			}
		}
		// Second tile: green; would change pixels if blended, but should be ignored due to early break
		let mut b = DynamicImage::new_rgba8(2, 2);
		for y in 0..2 {
			for x in 0..2 {
				b.put_pixel(x, y, imageproc::image::Rgba([0, 255, 0, 255]));
			}
		}

		let (res, idx) = stack_images(vec![(a.clone(), 1u8), (b, 2u8)]).unwrap().unwrap();
		assert_eq!(idx, Some(1u8));
		assert_eq!(res, a);
	}
}
