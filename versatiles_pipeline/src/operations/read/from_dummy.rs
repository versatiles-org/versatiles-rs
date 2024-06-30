#![allow(dead_code, unused_variables, unreachable_code)]

use crate::{
	traits::{OperationFactoryTrait, OperationTrait, ReadOperationFactoryTrait},
	vpl::VPLNode,
	PipelineFactory,
};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_core::types::{
	Blob, TileBBox, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat, TileStream,
	TilesReaderParameters,
};
use versatiles_image::helper::{create_dummy_image, image2blob};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Generates mocked tiles.
struct Args {
	/// Specifies the tile format.
	format: String,
	/// Compression type.
	compression: Option<String>,
	/// Minimum zoom level.
	zoom_min: Option<u8>,
	/// Maximum zoom level.
	zoom_max: Option<u8>,
	/// Minimum latitude.
	lat_min: Option<f32>,
	/// Maximum latitude.
	lat_max: Option<f32>,
	/// Minimum longitude.
	lng_min: Option<f32>,
	/// Maximum longitude.
	lng_max: Option<f32>,
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
}

impl<'a> Operation {
	fn build(vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn OperationTrait>>
	where
		Self: Sized + OperationTrait,
	{
		let args = Args::from_vpl_node(&vpl_node)?;

		let format = TileFormat::parse_str(&args.format)?;
		let compression = if let Some(c) = args.compression {
			TileCompression::parse_str(&c)?
		} else {
			TileCompression::Uncompressed
		};
		let bbox = TileBBoxPyramid::from_geo_bbox(
			args.zoom_min.unwrap_or(0),
			args.zoom_max.unwrap_or(12),
			&[
				args.lng_min.unwrap_or(-180.0) as f64,
				args.lat_min.unwrap_or(-90.0) as f64,
				args.lng_max.unwrap_or(180.0) as f64,
				args.lat_max.unwrap_or(90.0) as f64,
			],
		);

		let parameters = TilesReaderParameters::new(format, compression, bbox);

		Ok(Box::new(Self { parameters }))
	}
	fn build_tile(&self, coord: &TileCoord3) -> Option<Blob> {
		if self.parameters.bbox_pyramid.contains_coord(coord) {
			let format = self.parameters.tile_format;
			let blob = match format {
				TileFormat::JPG | TileFormat::PNG | TileFormat::WEBP => {
					let image = create_dummy_image(coord);
					image2blob(&image, format).unwrap()
				}
				_ => panic!("format '{format}' is not implemented yet"),
			};
			Some(blob)
		} else {
			None
		}
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn get_meta(&self) -> Option<Blob> {
		None
	}

	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		Ok(self.build_tile(coord))
	}

	async fn get_bbox_tile_stream(&self, mut bbox: TileBBox) -> TileStream {
		bbox.intersect_bbox(self.parameters.bbox_pyramid.get_level_bbox(bbox.level));
		let coords = bbox.iter_coords().collect::<Vec<TileCoord3>>();
		TileStream::from_coord_vec_sync(coords, |c| self.build_tile(&c).map(|b| (c, b)))
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"from_dummy"
	}
}

#[async_trait]
impl ReadOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>> {
		Operation::build(vpl_node, factory)
	}
}
