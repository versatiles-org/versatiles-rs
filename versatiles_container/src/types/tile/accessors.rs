//! Accessor methods for retrieving blob or content from a Tile.

use super::Tile;
use crate::TileContent;
use anyhow::{Result, anyhow};
use versatiles_core::{Blob, TileCompression, TileFormat};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::DynamicImage;

impl Tile {
	/// Get a reference to the encoded blob, re-(de)compressing as needed.
	///
	/// If no blob exists, the current content is encoded according to `self.format`,
	/// using any stored quality/speed hints. If a blob exists with a different
	/// `self.compression`, it is re-compressed to `compression`.
	///
	/// Returns a reference valid until the next mutating call.
	#[must_use = "this returns the blob reference, it doesn't modify anything externally"]
	#[context("getting blob (target_compression={:?})", compression)]
	pub fn as_blob(&mut self, compression: TileCompression) -> Result<&Blob> {
		self.materialize_blob()?;
		self.recompress_blob(compression)?;
		self.blob.as_ref().ok_or(anyhow!("blob should be present"))
	}

	#[context("accessing tile content")]
	pub(super) fn as_content(&mut self) -> Result<&TileContent> {
		self.materialize_content()?;
		self.content.as_ref().ok_or(anyhow!("content should be present"))
	}

	/// Borrow the raster image content, decoding the blob on demand.
	///
	/// Fails if the tile is not of a raster format.
	#[must_use = "this returns the image reference, it doesn't modify anything externally"]
	#[context("accessing raster image from tile")]
	pub fn as_image(&mut self) -> Result<&DynamicImage> {
		self.as_content()?.as_image()
	}

	/// Borrow the vector tile content, decoding the blob on demand.
	///
	/// Fails if the tile is not of a vector format.
	#[must_use = "this returns the vector reference, it doesn't modify anything externally"]
	#[context("accessing vector data from tile")]
	pub fn as_vector(&mut self) -> Result<&VectorTile> {
		self.as_content()?.as_vector()
	}

	/// Consume the tile and return an encoded blob with the requested compression.
	///
	/// This materializes content-to-blob if necessary and applies (re-)compression.
	#[must_use = "this consumes the tile and returns the blob"]
	#[context("converting tile into blob (target_compression={:?})", compression)]
	pub fn into_blob(mut self, compression: TileCompression) -> Result<Blob> {
		self.materialize_blob()?;
		self.recompress_blob(compression)?;
		Ok(self.blob.unwrap())
	}

	#[context("converting tile into content")]
	pub(super) fn into_content(mut self) -> Result<TileContent> {
		self.materialize_content()?;
		Ok(self.content.unwrap())
	}

	/// Consume the tile and return the owned raster image.
	///
	/// Fails if the tile is not a raster format.
	#[must_use = "this consumes the tile and returns the image"]
	#[context("converting tile into raster image")]
	pub fn into_image(self) -> Result<DynamicImage> {
		self.into_content()?.into_image()
	}

	/// Consume the tile and return the owned vector data.
	///
	/// Fails if the tile is not a vector format.
	#[must_use = "this consumes the tile and returns the vector data"]
	#[context("converting tile into vector data")]
	pub fn into_vector(self) -> Result<VectorTile> {
		self.into_content()?.into_vector()
	}

	#[context("accessing mutable tile content (dropping blob)")]
	pub(super) fn as_content_mut(&mut self) -> Result<&mut TileContent> {
		self.materialize_content()?;
		self.delete_blob();
		Ok(self.content.as_mut().unwrap())
	}

	/// Mutably borrow the raster image content.
	///
	/// Any existing blob is dropped because it would be stale after mutation.
	#[context("accessing mutable raster image (dropping blob)")]
	pub fn as_image_mut(&mut self) -> Result<&mut DynamicImage> {
		self.as_content_mut()?.as_image_mut()
	}

	/// Mutably borrow the vector content.
	///
	/// Any existing blob is dropped because it would be stale after mutation.
	#[context("accessing mutable vector data (dropping blob)")]
	pub fn as_vector_mut(&mut self) -> Result<&mut VectorTile> {
		self.as_content_mut()?.as_vector_mut()
	}

	/// Return the current tile **format** (e.g., `PNG`, `MVT`).
	#[must_use]
	pub fn format(&self) -> TileFormat {
		self.format
	}

	/// Return the current transport **compression** (e.g., `Uncompressed`, `Gzip`).
	#[must_use]
	pub fn compression(&self) -> TileCompression {
		self.compression
	}

	/// Whether the tile currently holds an encoded blob.
	#[must_use]
	pub fn has_blob(&self) -> bool {
		self.blob.is_some()
	}

	/// Whether the tile currently holds decoded content.
	#[must_use]
	pub fn has_content(&self) -> bool {
		self.content.is_some()
	}

	/// Set the encoding quality hint (used when re-encoding content to blob).
	///
	/// Pass `None` to clear any previously stored hint and use the codec default.
	pub fn set_format_quality(&mut self, quality: Option<u8>) {
		self.format_quality = quality;
	}

	/// Set the encoding speed hint (used when re-encoding content to blob).
	///
	/// Pass `None` to clear any previously stored hint and use the codec default.
	pub fn set_format_speed(&mut self, speed: Option<u8>) {
		self.format_speed = speed;
	}
}
