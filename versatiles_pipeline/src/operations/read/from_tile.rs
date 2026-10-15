//! # Single tile file source
//!
//! This operation reads a single tile file (PNG, WebP, JPEG, AVIF, or MVT/PBF)
//! and uses it as a template. All tile requests return clones of this tile.
//!
//! The tile format is automatically detected from the file extension, also for
//! a URL.
//!
//! ## Examples
//!
//! ```text
//! from_tile filename="background.png"
//! from_tile filename="water.webp"
//! from_tile filename="https://example.org/tiles/empty.pbf"
//! ```

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{Blob, TileBBox, TileCompression, TileFormat, TileJSON, TilePyramid, TileStream};

use crate::{PipelineFactory, helpers::location::TileFileLocation, vpl::VPLNode};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads one tile file and returns it for every requested coordinate.
struct Args {
	/// Path or URL of the tile file; its format comes from the extension.
	filename: TileFileLocation,
}

/// Implements [`TileSource`] by returning clones of a tile loaded from a file.
#[derive(Debug)]
pub struct Operation {
	tile: Tile,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
}

impl Operation {
	/// A source answering every coordinate with `blob`, read as `tile_format`.
	pub fn new(blob: Blob, tile_format: TileFormat) -> Self {
		let tile = Tile::from_blob(blob, TileCompression::Uncompressed, tile_format);

		let metadata = TileSourceMetadata::new(
			tile_format,
			TileCompression::Uncompressed,
			Traversal::ANY,
			Some(TilePyramid::new_full()),
		);

		let tilejson = {
			let mut tilejson = TileJSON::default();
			metadata.update_tilejson(&mut tilejson);
			tilejson
		};

		Self {
			tile,
			metadata,
			tilejson,
		}
	}

	#[cfg(test)]
	fn from_file(path: &std::path::Path) -> Result<Self> {
		let tile_format = TileFormat::try_from_path(path)?;
		Ok(Self::new(Blob::load_from_file(path)?, tile_format))
	}
}

impl Operation {
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>> {
		let args = Args::from_vpl_node(&vpl_node)?;
		let blob = args.filename.read_all(factory).await?;
		Ok(Box::new(Operation::new(blob, args.filename.format())))
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

	async fn tile(&self, _coord: &versatiles_core::TileCoord) -> Result<Option<Tile>> {
		Ok(Some(self.tile.clone()))
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("from_tile::tile_stream {bbox:?}");
		let tile = self.tile.clone();
		Ok(TileStream::from_bbox_parallel(bbox, move |_| Some(tile.clone())))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |_coord| {
			Some(())
		}))
	}
}

crate::operations::macros::define_read_factory!("from_tile", Args, Operation);

#[cfg(test)]
mod tests {
	use std::{fs::File, io::Write};

	use assert_fs::NamedTempFile;
	use versatiles_core::{TileCompression::Uncompressed, TileCoord};

	use super::*;

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
		assert_eq!(*op.metadata().tile_format(), TileFormat::PNG);
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
		let result = Operation::from_file(std::path::Path::new("/nonexistent/path/file.png"));
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn the_tile_can_come_from_a_url() -> Result<()> {
		let png = std::fs::read(create_temp_png().path())?;
		let base = crate::helpers::test_server::serve_files(vec![("/water.png", png.clone())]).await;
		let op = PipelineFactory::new_dummy()
			.operation_from_vpl(&format!("from_tile filename=\"{base}/water.png?key=abc\""))
			.await?;
		assert_eq!(*op.metadata().tile_format(), TileFormat::PNG);
		let tile = op.tile(&TileCoord::new(3, 1, 2)?).await?.unwrap();
		assert_eq!(tile.into_blob(&Uncompressed)?.as_slice(), png.as_slice());
		Ok(())
	}

	#[tokio::test]
	async fn test_tile() {
		let temp_file = create_temp_png();
		let op = Operation::from_file(temp_file.path()).unwrap();
		let tile = op.tile(&TileCoord::new(0, 0, 0).unwrap()).await.unwrap();
		assert!(tile.is_some());
		let blob = tile.unwrap().into_blob(&Uncompressed).unwrap();
		assert!(!blob.is_empty());
	}

	#[tokio::test]
	async fn test_tile_stream() {
		let temp_file = create_temp_png();
		let op = Operation::from_file(temp_file.path()).unwrap();
		let bbox = TileBBox::from_min_and_max(2, 0, 0, 1, 1).unwrap();
		let mut stream = op.tile_stream(bbox).await.unwrap();

		let mut count = 0;
		while let Some((coord, tile)) = stream.next().await {
			assert_eq!(coord.level, 2);
			assert!(!tile.into_blob(&Uncompressed).unwrap().is_empty());
			count += 1;
		}
		assert_eq!(count, 4); // 2x2 tiles at level 2
	}

	#[tokio::test]
	async fn test_tilejson() {
		let temp_file = create_temp_png();
		let op = Operation::from_file(temp_file.path()).unwrap();
		let tilejson = op.tilejson();
		assert!(tilejson.to_pretty_lines(100).join("\n").contains("image/png"));
	}

	#[test]
	fn test_source_type() {
		let temp_file = create_temp_png();
		let op = Operation::from_file(temp_file.path()).unwrap();
		let source_type = op.source_type();
		assert!(format!("{source_type:?}").contains("tile"));
	}
}
