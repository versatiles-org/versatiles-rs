mod image;
mod vector;

use crate::{traits::*, vpl::VPLNode, PipelineFactory};
use anyhow::{bail, Result};
use async_trait::async_trait;
use futures::future::BoxFuture;
use image::create_debug_image;
use std::fmt::Debug;
use vector::create_debug_vector_tile;
use versatiles_core::types::{
	Blob, TileBBox, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat, TileStream,
	TilesReaderParameters,
};
use versatiles_image::helper::image2blob;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Produces debugging tiles, showing their coordinates.
struct Args {
	/// tile format: "pbf", "jpg", "png" or "webp"
	format: String,
}

#[derive(Debug)]
struct Operation {
	meta: Option<Blob>,
	parameters: TilesReaderParameters,
}

impl Operation {
	fn build_tile(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		Ok(if self.parameters.bbox_pyramid.contains_coord(coord) {
			let format = self.parameters.tile_format;
			let blob = match format {
				TileFormat::JPG | TileFormat::PNG | TileFormat::WEBP => {
					image2blob(&create_debug_image(coord), format)?
				}
				TileFormat::PBF => create_debug_vector_tile(coord)?,
				_ => bail!("tile format '{format}' is not implemented yet"),
			};
			Some(blob)
		} else {
			None
		})
	}
}

impl ReadOperationTrait for Operation {
	fn build(
		vpl_node: VPLNode,
		_factory: &PipelineFactory,
	) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;

			let format = TileFormat::parse_str(&args.format)?;
			let parameters = TilesReaderParameters::new(
				format,
				TileCompression::Uncompressed,
				TileBBoxPyramid::new_full(31),
			);

			let meta = Some(match format {
				TileFormat::PBF => Blob::from(format!(
					"{{\"vector_layers\":[{}]}}",
					["background", "debug_x", "debug_y", "debug_z"]
						.map(|n| format!("{{\"id\":\"{n}\",\"minzoom\":0,\"maxzoom\":31}}"))
						.join(",")
				)),
				_ => Blob::from("{}"),
			});

			Ok(Box::new(Self { meta, parameters }) as Box<dyn OperationTrait>)
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn get_meta(&self) -> Option<Blob> {
		self.meta.clone()
	}

	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		self.build_tile(coord)
	}

	async fn get_bbox_tile_stream(&self, mut bbox: TileBBox) -> TileStream {
		bbox.intersect_pyramid(&self.parameters.bbox_pyramid);
		let coords = bbox.iter_coords().collect::<Vec<TileCoord3>>();
		TileStream::from_coord_vec_sync(coords, |c| {
			self.build_tile(&c).ok().flatten().map(|b| (c, b))
		})
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"from_debug"
	}
}

#[async_trait]
impl ReadOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>> {
		Operation::build(vpl_node, factory).await
	}
}