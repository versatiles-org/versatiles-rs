//! Internal conversion methods for blob/content materialization and compression.

use super::Tile;
use anyhow::{Result, ensure};
use versatiles_core::{
	TileCompression,
	compression::{decompress_ref, recompress},
};
use versatiles_derive::context;

use crate::TileContent;

impl Tile {
	#[context("recompressing blob: {:?} -> {:?}", self.compression, compression)]
	pub(super) fn recompress_blob(&mut self, compression: TileCompression) -> Result<()> {
		assert!(self.blob.is_some());
		if self.compression != compression {
			self.blob = Some(recompress(self.blob.take().unwrap(), self.compression, compression)?);
			self.compression = compression;
		}
		Ok(())
	}

	#[context("decompressing blob ({:?})", self.compression)]
	pub(super) fn decompress_blob(&mut self) -> Result<()> {
		assert!(self.blob.is_some());
		if self.compression != TileCompression::Uncompressed {
			self.blob = Some(decompress_ref(self.blob.as_ref().unwrap(), self.compression)?);
			self.compression = TileCompression::Uncompressed;
		}
		Ok(())
	}

	#[cfg(test)]
	pub(super) fn __recompress_blob_for_test(&mut self, compression: TileCompression) {
		self.recompress_blob(compression).unwrap();
	}

	#[cfg(test)]
	pub(super) fn __decompress_blob_for_test(&mut self) {
		self.decompress_blob().unwrap();
	}

	pub(super) fn delete_blob(&mut self) {
		self.blob = None;
		self.compression = TileCompression::Uncompressed;
	}

	#[context("materializing blob from content (format={:?}, q={:?}, s={:?})", self.format, self.format_quality, self.format_speed)]
	pub(super) fn materialize_blob(&mut self) -> Result<()> {
		if self.blob.is_none() {
			ensure!(self.content.is_some(), "Cannot materialize blob without content");
			self.blob = Some(self.content.as_ref().unwrap().to_blob(
				self.format,
				self.format_quality,
				self.format_speed,
			)?);
			self.compression = TileCompression::Uncompressed;
		}
		Ok(())
	}

	#[context("materializing content from blob (format={:?})", self.format)]
	pub(super) fn materialize_content(&mut self) -> Result<()> {
		if self.content.is_none() {
			ensure!(self.blob.is_some(), "Cannot materialize content without blob");
			self.decompress_blob()?;
			self.content = Some(TileContent::from_blob(self.blob.as_ref().unwrap(), self.format)?);
		}
		Ok(())
	}

	/// Change the tile's **format** (e.g., `PNG` → `WEBP`) while preserving the content type.
	///
	/// The tile's content is materialized; the existing blob is dropped; and optional
	/// `quality`/`speed` hints are updated if provided (passed as `Some`).
	/// Passing `None` keeps the previous hint value.
	///
	/// The `format` must have the same type (raster vs. vector) as the current format.
	#[context("changing format: {:?} -> {:?} (q={:?}, s={:?})", self.format, format, quality, speed)]
	pub fn change_format(
		&mut self,
		format: versatiles_core::TileFormat,
		quality: Option<u8>,
		speed: Option<u8>,
	) -> Result<()> {
		if self.format == format && quality.is_none() {
			return Ok(());
		}

		assert_eq!(format.to_type(), self.format.to_type());
		self.materialize_content()?;
		self.delete_blob();
		self.compression = TileCompression::Uncompressed;
		self.format = format;

		if quality.is_some() {
			self.format_quality = quality;
		}
		if speed.is_some() {
			self.format_speed = speed;
		}
		Ok(())
	}

	/// Update the outer **compression** flag and (if a blob is present) re-compress it.
	///
	/// When no blob is present, only the flag changes; compression is applied on the
	/// next materialization of the blob.
	#[context("changing compression to {:?}", compression)]
	pub fn change_compression(&mut self, compression: TileCompression) -> Result<()> {
		if self.blob.is_some() {
			self.recompress_blob(compression)?;
		} else {
			self.compression = compression;
		}
		Ok(())
	}
}
