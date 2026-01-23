//! # Single tile file source
//!
//! This operation reads a single tile file (PNG, WebP, JPEG, AVIF, or MVT/PBF)
//! and uses it as a template. All tile requests return clones of this tile.
//!
//! The tile format is automatically detected from the file extension.
//!
//! ## Examples
//!
//! ```text
//! from_tile filename="background.png"
//! from_tile filename="water.webp"
//! from_tile filename="empty.pbf"
//! ```

use crate::{PipelineFactory, operations::read::traits::ReadTileSource, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use std::{path::Path, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{Blob, TileBBox, TileBBoxPyramid, TileCompression, TileFormat, TileJSON, TileStream};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a single tile file and uses it as a template for all tile requests.
struct Args {
	/// The filename of the tile. Supported formats: png, jpg/jpeg, webp, avif, pbf/mvt.
	/// The format is automatically detected from the file extension.
	filename: String,
}

/// Implements [`TileSource`] by returning clones of a tile loaded from a file.
#[derive(Debug)]
pub struct Operation {
	tile: Tile,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
}

impl Operation {
	pub fn from_file(path: &Path) -> Result<Self> {
		let tile_format = TileFormat::try_from_path(path)?;
		let blob = Blob::load_from_file(path)?;
		let tile = Tile::from_blob(blob, TileCompression::Uncompressed, tile_format);

		let metadata = TileSourceMetadata::new(
			tile_format,
			TileCompression::Uncompressed,
			TileBBoxPyramid::new_full(),
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
}

impl ReadTileSource for Operation {
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let path = factory.resolve_path(&args.filename);
		Operation::from_file(&path).map(|op| Box::new(op) as Box<dyn TileSource>)
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
		SourceType::new_container("tile file", "tile")
	}

	async fn get_tile(&self, _coord: &versatiles_core::TileCoord) -> Result<Option<Tile>> {
		Ok(Some(self.tile.clone()))
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let tile = self.tile.clone();
		Ok(TileStream::from_bbox_parallel(bbox, move |_| Some(tile.clone())))
	}
}

crate::operations::macros::define_read_factory!("from_tile", Args, Operation);

#[cfg(test)]
mod tests {
	use super::*;
	use assert_fs::NamedTempFile;
	use std::{fs::File, io::Write};
	use versatiles_core::{TileCompression::Uncompressed, TileCoord};

	fn create_temp_png() -> NamedTempFile {
		// Minimal valid PNG: 1x1 pixel, red
		let png_data: &[u8] = &[
			0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
			0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
			0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
			0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, // 8-bit RGB
			0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, // IDAT chunk
			0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x01, 0x01, 0x01, 0x00, // compressed data
			0x1B, 0xB6, 0xEE, 0x56, // CRC
			0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, // IEND chunk
			0xAE, 0x42, 0x60, 0x82, // CRC
		];

		let temp_file = NamedTempFile::new("test.png").unwrap();
		let mut file = File::create(temp_file.path()).unwrap();
		file.write_all(png_data).unwrap();
		temp_file
	}

	#[test]
	fn test_from_file_png() {
		let temp_file = create_temp_png();
		let op = Operation::from_file(temp_file.path()).unwrap();
		assert_eq!(op.metadata().tile_format, TileFormat::PNG);
	}

	#[test]
	fn test_from_file_invalid_extension() {
		let temp_file = NamedTempFile::new("test.xyz").unwrap();
		File::create(temp_file.path()).unwrap();
		let result = Operation::from_file(temp_file.path());
		assert!(result.is_err());
	}

	#[test]
	fn test_from_file_nonexistent() {
		let result = Operation::from_file(Path::new("/nonexistent/path/file.png"));
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_get_tile() {
		let temp_file = create_temp_png();
		let op = Operation::from_file(temp_file.path()).unwrap();
		let tile = op.get_tile(&TileCoord::new(0, 0, 0).unwrap()).await.unwrap();
		assert!(tile.is_some());
		let blob = tile.unwrap().into_blob(Uncompressed).unwrap();
		assert!(!blob.is_empty());
	}

	#[tokio::test]
	async fn test_get_tile_stream() {
		let temp_file = create_temp_png();
		let op = Operation::from_file(temp_file.path()).unwrap();
		let bbox = TileBBox::from_min_and_max(2, 0, 0, 1, 1).unwrap();
		let mut stream = op.get_tile_stream(bbox).await.unwrap();

		let mut count = 0;
		while let Some((coord, tile)) = stream.next().await {
			assert!(coord.level == 2);
			assert!(!tile.into_blob(Uncompressed).unwrap().is_empty());
			count += 1;
		}
		assert_eq!(count, 4); // 2x2 tiles at level 2
	}

	#[tokio::test]
	async fn test_tilejson() {
		let temp_file = create_temp_png();
		let op = Operation::from_file(temp_file.path()).unwrap();
		let tilejson = op.tilejson();
		assert!(tilejson.as_pretty_lines(100).join("\n").contains("image/png"));
	}

	#[test]
	fn test_source_type() {
		let temp_file = create_temp_png();
		let op = Operation::from_file(temp_file.path()).unwrap();
		let source_type = op.source_type();
		assert!(format!("{source_type:?}").contains("tile"));
	}
}
