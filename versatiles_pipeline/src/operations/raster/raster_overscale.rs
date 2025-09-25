use crate::{PipelineFactory, helpers::pack_image_tile_stream, traits::*, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::DynamicImage;
use std::fmt::Debug;
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::traits::*;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filter tiles by bounding box and/or zoom levels.
struct Args {
	/// use this zoom level to build the overscale. Defaults to the maximum zoom level of the source.
	level_base: Option<u8>,
	/// use this as maximum zoom level. Defaults to 30.
	level_max: Option<u8>,
	/// Size of the tiles in pixels. Defaults to 512.
	tile_size: Option<u32>,
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	source: Box<dyn OperationTrait>,
	tilejson: TileJSON,
	level_base: u8,
	tile_size: u32,
}

impl Operation {
	fn build(
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		_factory: &PipelineFactory,
	) -> BoxFuture<'_, Result<Box<dyn OperationTrait>, anyhow::Error>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			let mut parameters = source.parameters().clone();

			let level_base = args
				.level_base
				.unwrap_or(source.parameters().bbox_pyramid.get_zoom_max().unwrap());
			log::trace!("level_base {}", level_base);

			let level_max = args.level_max.unwrap_or(30).clamp(level_base, 30);

			let mut level_bbox = *parameters.bbox_pyramid.get_level_bbox(level_base);
			while level_bbox.level <= level_max {
				level_bbox.level_up();
				parameters.bbox_pyramid.set_level_bbox(level_bbox);
			}

			let mut tilejson = source.tilejson().clone();
			tilejson.update_from_reader_parameters(&parameters);

			Ok(Box::new(Self {
				parameters,
				source,
				tilejson,
				level_base,
				tile_size: args.tile_size.unwrap_or(512),
			}) as Box<dyn OperationTrait>)
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn traversal(&self) -> &Traversal {
		self.source.traversal()
	}

	async fn get_image_stream(&self, bbox_dst: TileBBox) -> Result<TileStream<DynamicImage>> {
		log::debug!("get_image_stream {:?}", bbox_dst);

		if !self.parameters.bbox_pyramid.overlaps_bbox(&bbox_dst) {
			log::trace!("get_image_stream outside bbox_pyramid");
			return Ok(TileStream::new_empty());
		}

		if bbox_dst.level <= self.level_base {
			log::trace!("get_image_stream level <= level_base");
			return self.source.get_image_stream(bbox_dst).await;
		}

		let level_dst = bbox_dst.level;

		let bbox_base = bbox_dst.at_level(self.level_base);
		let stream_base = self.source.get_image_stream(bbox_base).await?;

		let tile_size = self.tile_size;
		let tile_size_f64 = tile_size as f64;
		let scale = (1 << (bbox_dst.level - self.level_base)) as f64;
		let s = tile_size_f64 / scale;

		Ok(stream_base.flat_map_parallel(move |coord_base, image_base| {
			let mut bbox = coord_base.as_tile_bbox(1).unwrap().at_level(level_dst);
			bbox.intersect_with(&bbox_dst).unwrap();

			Ok(TileStream::from_iter_coord_parallel(
				bbox.into_iter_coords(),
				move |coord| {
					let x0 = coord.x as f64 * s - (coord_base.x as f64 * tile_size_f64);
					let y0 = coord.y as f64 * s - (coord_base.y as f64 * tile_size_f64);

					let image = image_base.get_extract(x0, y0, s, s, tile_size, tile_size).unwrap();
					image.into_optional()
				},
			))
		}))
	}

	async fn get_blob_stream(&self, bbox: TileBBox) -> Result<TileStream<Blob>> {
		log::debug!("get_blob_stream {:?}", bbox);

		if bbox.level >= self.level_base {
			return self.source.get_blob_stream(bbox).await;
		}
		pack_image_tile_stream(self.get_image_stream(bbox).await, &self.parameters)
	}

	async fn get_vector_stream(&self, _bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		bail!("Vector tiles are not supported in raster_overscale operations.");
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"raster_overscale"
	}
}

#[async_trait]
impl TransformOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>> {
		Operation::build(vpl_node, source, factory).await
	}
}

#[cfg(test)]
mod tests {}
