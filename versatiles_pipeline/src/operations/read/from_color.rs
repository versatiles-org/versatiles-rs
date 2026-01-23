//! # Solid color tile generator
//!
//! This operation produces solid-color raster tiles. It creates a single
//! template tile of the specified color, size, and format, and returns
//! clones of this tile for all coordinates in the requested bounding box.
//!
//! ## Examples
//!
//! ```text
//! from_color color=FF5733 size=512 format=png
//! from_color color=FF573380 size=256 format=webp
//! from_color  # defaults: color=000000 size=512 format=png
//! ```

use crate::{PipelineFactory, operations::read::traits::ReadTileSource, vpl::VPLNode};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use std::sync::Arc;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{TileBBox, TileBBoxPyramid, TileCompression, TileFormat, TileJSON, TileStream};
use versatiles_image::{DynamicImageTraitConvert, color::parse_hex_color};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Generates solid-color tiles of the specified size and format.
struct Args {
	/// Hex color in RGB or RGBA format (e.g., "FF5733" or "FF573380"). Defaults to "000000" (black).
	color: Option<String>,
	/// Tile size in pixels (256 or 512). Defaults to 512.
	size: Option<u16>,
	/// Tile format: one of "avif", "jpg", "png", or "webp". Defaults to "png".
	format: Option<String>,
}

/// Implements [`TileSource`] by returning clones of a pre-generated solid-color tile.
#[derive(Debug)]
pub struct Operation {
	tile: Tile,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
}

impl Operation {
	pub fn from_parameters(color: &[u8], tile_size: u32, tile_format: TileFormat) -> Result<Self> {
		ensure!(
			tile_size == 256 || tile_size == 512,
			"tile size must be 256 or 512, got {tile_size}"
		);
		ensure!(
			tile_format.is_raster(),
			"tile format must be a raster format (avif, jpg, png, webp), got {tile_format}"
		);

		let data = std::iter::repeat_n(color.to_vec(), (tile_size * tile_size) as usize)
			.flatten()
			.collect::<Vec<u8>>();
		let image = versatiles_image::DynamicImage::from_raw(tile_size as usize, tile_size as usize, data)?;
		let blob = Tile::from_image(image, tile_format)?.into_blob(TileCompression::Uncompressed)?;
		let tile = Tile::from_blob(blob, TileCompression::Uncompressed, tile_format);

		let metadata = TileSourceMetadata::new(
			tile_format,
			TileCompression::Uncompressed,
			TileBBoxPyramid::new_full(30),
			Traversal::ANY,
		);

		let tilejson = {
			let mut tilejson = TileJSON::default();
			metadata.update_tilejson(&mut tilejson);
			tilejson
		};

		Ok(Self {
			tile,
			metadata,
			tilejson,
		})
	}

	pub fn from_vpl_node(vpl_node: &VPLNode) -> Result<Self> {
		let args = Args::from_vpl_node(vpl_node)?;

		let color = args.color.as_deref().unwrap_or("000000");
		let color_bytes = parse_hex_color(color)?;

		let tile_size = u32::from(args.size.unwrap_or(512));

		let tile_format = args
			.format
			.map(|f| TileFormat::try_from_str(&f))
			.transpose()?
			.unwrap_or(TileFormat::PNG);

		Self::from_parameters(&color_bytes, tile_size, tile_format)
	}
}

impl ReadTileSource for Operation {
	async fn build(vpl_node: VPLNode, _factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized + TileSource,
	{
		Operation::from_vpl_node(&vpl_node).map(|op| Box::new(op) as Box<dyn TileSource>)
	}
}

#[async_trait]
impl TileSource for Operation {
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("solid color", "color")
	}

	async fn get_tile(&self, _coord: &versatiles_core::TileCoord) -> Result<Option<Tile>> {
		Ok(Some(self.tile.clone()))
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let tile = self.tile.clone();
		Ok(TileStream::from_bbox_parallel(bbox, move |_| Some(tile.clone())))
	}
}

crate::operations::macros::define_read_factory!("from_color", Args, Operation);

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::{TileCompression::Uncompressed, TileCoord};

	#[test]
	fn test_operation_default_parameters() {
		let op = Operation::from_parameters(&[0, 0, 0], 512, TileFormat::PNG).unwrap();
		assert_eq!(op.metadata().tile_format, TileFormat::PNG);
	}

	#[test]
	fn test_operation_invalid_size() {
		assert!(Operation::from_parameters(&[0, 0, 0], 128, TileFormat::PNG).is_err());
		assert!(Operation::from_parameters(&[0, 0, 0], 1024, TileFormat::PNG).is_err());
	}

	#[test]
	fn test_operation_invalid_format() {
		assert!(Operation::from_parameters(&[0, 0, 0], 512, TileFormat::MVT).is_err());
	}

	#[tokio::test]
	async fn test_operation_get_tile() {
		let op = Operation::from_parameters(&[255, 0, 0], 256, TileFormat::PNG).unwrap();
		let tile = op.get_tile(&TileCoord::new(0, 0, 0).unwrap()).await.unwrap();
		assert!(tile.is_some());
		let blob = tile.unwrap().into_blob(Uncompressed).unwrap();
		assert!(!blob.is_empty());
	}

	#[tokio::test]
	async fn test_from_vpl() {
		let factory = PipelineFactory::new_dummy();

		// Test with all parameters
		let op = factory
			.operation_from_vpl("from_color color=FF5733 size=256 format=webp")
			.await
			.unwrap();
		assert_eq!(op.metadata().tile_format, TileFormat::WEBP);

		// Test with defaults
		let op = factory.operation_from_vpl("from_color").await.unwrap();
		assert_eq!(op.metadata().tile_format, TileFormat::PNG);

		// Test tile content
		let coord = TileCoord::new(5, 10, 15).unwrap();
		let tile = op
			.get_tile_stream(coord.to_tile_bbox())
			.await
			.unwrap()
			.next()
			.await
			.unwrap()
			.1;
		assert!(!tile.into_blob(Uncompressed).unwrap().is_empty());
	}

	#[tokio::test]
	async fn test_tilejson() {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_color color=00FF00 format=png")
			.await
			.unwrap();

		let tilejson = op.tilejson();
		assert!(tilejson.as_pretty_lines(100).join("\n").contains("image/png"));
	}
}
