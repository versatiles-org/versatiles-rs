use crate::{PipelineFactory, traits::*, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::DynamicImage;
use std::{fmt::Debug, str};
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_geometry::vector_tile::VectorTile;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filter tiles by bounding box and/or zoom levels.
struct Args {
	/// The desired tile format. Allowed values are: AVIF, JPG, PNG or WEBP.
	/// If not specified, the source format will be used.
	format: Option<String>,
	/// Quality level for the tile compression (only AVIF, JPG or WEBP), between 0 (worst) and 100 (lossless).
	quality: Option<u8>,
	/// Compression speed (only AVIF), between 0 (slowest) and 100 (fastest).
	speed: Option<u8>,
}

#[derive(Debug, Clone, Copy)]
enum RasterTileFormat {
	Avif,
	Jpeg,
	Png,
	Webp,
}

impl RasterTileFormat {
	fn from_str(text: &str) -> Result<Self> {
		use RasterTileFormat::*;
		Ok(match text.to_lowercase().trim() {
			"avif" => Avif,
			"jpg" | "jpeg" => Jpeg,
			"png" => Png,
			"webp" => Webp,
			_ => bail!("Invalid tile format '{text}'"),
		})
	}
}

impl TryFrom<&TileFormat> for RasterTileFormat {
	type Error = anyhow::Error;
	fn try_from(value: &TileFormat) -> std::result::Result<Self, Self::Error> {
		use RasterTileFormat::*;
		Ok(match value {
			TileFormat::AVIF => Avif,
			TileFormat::JPG => Jpeg,
			TileFormat::PNG => Png,
			TileFormat::WEBP => Webp,
			_ => bail!("Invalid tile format '{value}' for raster operations"),
		})
	}
}

impl From<RasterTileFormat> for TileFormat {
	fn from(value: RasterTileFormat) -> Self {
		use RasterTileFormat::*;
		match value {
			Avif => TileFormat::AVIF,
			Jpeg => TileFormat::JPG,
			Png => TileFormat::PNG,
			Webp => TileFormat::WEBP,
		}
	}
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	source: Box<dyn OperationTrait>,
	tilejson: TileJSON,
	format: RasterTileFormat,
	quality: Option<u8>,
	speed: Option<u8>,
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

			let format: RasterTileFormat = if let Some(text) = args.format {
				RasterTileFormat::from_str(&text)?
			} else {
				RasterTileFormat::try_from(&parameters.tile_format)?
			};

			parameters.tile_format = format.into();
			parameters.tile_compression = TileCompression::Uncompressed;

			let mut tilejson = source.tilejson().clone();
			tilejson.update_from_reader_parameters(&parameters);

			Ok(Box::new(Self {
				format,
				quality: args.quality,
				speed: args.speed,
				parameters,
				source,
				tilejson,
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

	async fn get_image_stream(&self, _bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		bail!(
			"Operation 'raster_format' must be the last operation in a pipeline and cannot be used as an image source."
		);
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Blob>> {
		use RasterTileFormat::*;
		use versatiles_image::{avif, jpeg, png, webp};

		let quality = self.quality;
		let speed = self.speed;
		let stream = self.source.get_image_stream(bbox).await?;

		Ok(match self.format {
			Avif => stream.map_item_parallel(move |image| avif::compress(&image, quality, speed)),
			Jpeg => stream.map_item_parallel(move |image| jpeg::compress(&image, quality)),
			Png => stream.map_item_parallel(move |image| png::compress(&image, speed)),
			Webp => stream.map_item_parallel(move |image| webp::compress(&image, quality)),
		})
	}

	async fn get_vector_stream(&self, _bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		bail!("Vector tiles are not supported in raster_format operations.");
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"raster_format"
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
