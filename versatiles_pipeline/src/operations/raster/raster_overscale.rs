use crate::{PipelineFactory, traits::*, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_container::Tile;
use versatiles_core::*;
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
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		_factory: &PipelineFactory,
	) -> Result<Box<dyn OperationTrait>>
	where
		Self: Sized + OperationTrait,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let mut parameters = source.parameters().clone();

		let level_base = args
			.level_base
			.unwrap_or(source.parameters().bbox_pyramid.get_level_max().unwrap());
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

	async fn get_stream(&self, bbox_dst: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_stream {:?}", bbox_dst);

		if !self.parameters.bbox_pyramid.overlaps_bbox(&bbox_dst) {
			log::trace!("get_stream outside bbox_pyramid");
			return Ok(TileStream::empty());
		}

		if bbox_dst.level <= self.level_base {
			log::trace!("get_stream level <= level_base");
			return self.source.get_stream(bbox_dst).await;
		}

		let level_dst = bbox_dst.level;

		let bbox_base = bbox_dst.at_level(self.level_base);
		let stream_base = self.source.get_stream(bbox_base).await?;

		let tile_size = self.tile_size;
		let tile_size_f64 = tile_size as f64;
		let scale = (1 << (bbox_dst.level - self.level_base)) as f64;
		let s = tile_size_f64 / scale;
		let format = self.source.parameters().tile_format;

		Ok(stream_base.flat_map_parallel(move |coord_base, tile_src| {
			let mut bbox = coord_base.as_tile_bbox(1)?.at_level(level_dst);
			bbox.intersect_with(&bbox_dst)?;

			let image_src = tile_src.into_image()?;

			Ok(TileStream::from_iter_coord_parallel(
				bbox.into_iter_coords(),
				move |coord| {
					let x0 = coord.x as f64 * s - (coord_base.x as f64 * tile_size_f64);
					let y0 = coord.y as f64 * s - (coord_base.y as f64 * tile_size_f64);

					let image_dst = image_src.get_extract(x0, y0, s, s, tile_size, tile_size).unwrap();

					image_dst
						.into_optional()
						.map(|image_dst| Tile::from_image(image_dst, format).unwrap())
				},
			))
		}))
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
