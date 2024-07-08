mod image;
mod vector;

use crate::{
	image::helper::image2blob,
	traits::*,
	types::{
		Blob, TileBBox, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat, TileStream,
		TilesReaderParameters,
	},
	vpl::VPLNode,
	PipelineFactory,
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use futures::future::BoxFuture;
use image::create_debug_image;
use std::fmt::Debug;
use vector::create_debug_vector_tile;
use versatiles_image::helper::image2blob_fast;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Produces debugging tiles, each showing their coordinates as text.
struct Args {
	/// tile format: "pbf", "jpg", "png" or "webp"
	format: String,
	/// use fast compression
	fast: bool,
}

#[derive(Debug)]
pub struct Operation {
	meta: Option<Blob>,
	parameters: TilesReaderParameters,
	fast_compression: bool,
}

impl Operation {
	pub fn from_parameters(
		tile_format: TileFormat,
		fast_compression: bool,
	) -> Result<Box<dyn OperationTrait>> {
		let parameters = TilesReaderParameters::new(
			tile_format,
			TileCompression::Uncompressed,
			TileBBoxPyramid::new_full(31),
		);

		let meta = Some(match tile_format {
			TileFormat::PBF => Blob::from(format!(
				"{{\"vector_layers\":[{}]}}",
				["background", "debug_x", "debug_y", "debug_z"]
					.map(|n| format!("{{\"id\":\"{n}\",\"minzoom\":0,\"maxzoom\":31}}"))
					.join(",")
			)),
			_ => Blob::from("{}"),
		});

		Ok(Box::new(Self {
			meta,
			parameters,
			fast_compression,
		}) as Box<dyn OperationTrait>)
	}
	pub fn from_vpl_node(vpl_node: &VPLNode) -> Result<Box<dyn OperationTrait>> {
		let args = Args::from_vpl_node(vpl_node)?;
		Self::from_parameters(TileFormat::parse_str(&args.format)?, args.fast)
	}
}

fn build_tile(
	coord: &TileCoord3,
	format: TileFormat,
	fast_compression: bool,
) -> Result<Option<Blob>> {
	Ok(Some(match format {
		TileFormat::JPG | TileFormat::PNG | TileFormat::WEBP => {
			let image = create_debug_image(coord);
			if fast_compression {
				image2blob_fast(&image, format)?
			} else {
				image2blob(&image, format)?
			}
		}
		TileFormat::PBF => create_debug_vector_tile(coord)?,
		_ => bail!("tile format '{format}' is not implemented yet"),
	}))
}

impl ReadOperationTrait for Operation {
	fn build(
		vpl_node: VPLNode,
		_factory: &PipelineFactory,
	) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move { Operation::from_vpl_node(&vpl_node) })
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
		build_tile(coord, self.parameters.tile_format, self.fast_compression)
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		let format = self.parameters.tile_format;
		let fast = self.fast_compression;

		TileStream::from_coord_iter_parallel(bbox.into_iter_coords(), move |c| {
			build_tile(&c, format, fast).ok().flatten()
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

#[cfg(test)]
mod tests {
	use super::*;

	async fn test(format: &str, len: u64, meta: &str) -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let mut operation = factory
			.operation_from_vpl(&format!("from_debug format={format}"))
			.await?;

		let coord = TileCoord3 { x: 1, y: 2, z: 3 };
		let blob = operation.get_tile_data(&coord).await?.unwrap();

		assert_eq!(blob.len(), len, "for '{format}'");
		assert_eq!(
			operation.get_meta().unwrap().as_str(),
			meta,
			"for '{format}'"
		);

		let mut stream = operation
			.get_bbox_tile_stream(TileBBox::new(3, 1, 1, 2, 3)?)
			.await;

		let mut n = 0;
		while let Some((coord, blob)) = stream.next().await {
			assert!(!blob.is_empty(), "for '{format}'");
			assert!(coord.x >= 1 && coord.x <= 2, "for '{format}'");
			assert!(coord.y >= 1 && coord.y <= 3, "for '{format}'");
			assert_eq!(coord.z, 3, "for '{format}'");
			n += 1;
		}
		assert_eq!(n, 6, "for '{format}'");

		Ok(())
	}

	#[tokio::test]
	async fn test_build_tile_png() {
		test("png", 5207, "{}").await.unwrap();
	}

	#[tokio::test]
	async fn test_build_tile_jpg() {
		test("jpg", 11808, "{}").await.unwrap();
	}

	#[tokio::test]
	async fn test_build_tile_webp() {
		test("webp", 2656, "{}").await.unwrap();
	}

	#[tokio::test]
	async fn test_build_tile_vector() {
		test("pbf", 1732, "{\"vector_layers\":[{\"id\":\"background\",\"minzoom\":0,\"maxzoom\":31},{\"id\":\"debug_x\",\"minzoom\":0,\"maxzoom\":31},{\"id\":\"debug_y\",\"minzoom\":0,\"maxzoom\":31},{\"id\":\"debug_z\",\"minzoom\":0,\"maxzoom\":31}]}").await.unwrap();
	}
}
