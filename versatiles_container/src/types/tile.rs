use std::fmt::Debug;
use versatiles_core::{
	Blob, TileCompression, TileFormat,
	cache::CacheValue,
	utils::{decompress_ref, recompress},
};
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::DynamicImage;

use crate::TileContent;

#[derive(Clone, PartialEq)]
pub struct Tile {
	blob: Option<Blob>,
	content: Option<TileContent>,
	format: TileFormat,
	compression: TileCompression,
	format_quality: Option<u8>,
	format_speed: Option<u8>,
}

impl Tile {
	pub fn from_blob(blob: Blob, compression: TileCompression, format: TileFormat) -> Self {
		Self {
			blob: Some(blob),
			content: None,
			format,
			compression,
			format_quality: None,
			format_speed: None,
		}
	}
	fn from_content(content: TileContent, format: TileFormat) -> Self {
		Self {
			blob: None,
			content: Some(content),
			format,
			compression: TileCompression::Uncompressed,
			format_quality: None,
			format_speed: None,
		}
	}
	pub fn from_image(image: DynamicImage, format: TileFormat) -> Self {
		Self::from_content(TileContent::from_image(image, format), format)
	}
	pub fn from_vector(vector_tile: versatiles_geometry::vector_tile::VectorTile, format: TileFormat) -> Self {
		Self::from_content(TileContent::from_vector(vector_tile, format), format)
	}

	fn recompress_blob(&mut self, compression: TileCompression) {
		assert!(self.blob.is_some());
		if self.compression != compression {
			self.blob = Some(recompress(self.blob.take().unwrap(), self.compression, compression).unwrap());
			self.compression = compression;
		}
	}
	fn decompress_blob(&mut self) {
		assert!(self.blob.is_some());
		if self.compression != TileCompression::Uncompressed {
			self.blob = Some(decompress_ref(self.blob.as_ref().unwrap(), self.compression).unwrap());
			self.compression = TileCompression::Uncompressed;
		}
	}
	fn delete_blob(&mut self) {
		self.blob = None;
		self.compression = TileCompression::Uncompressed;
	}
	fn materialize_blob(&mut self) {
		if self.blob.is_none() {
			assert!(self.content.is_some());
			self.blob = Some(
				self
					.content
					.as_ref()
					.unwrap()
					.to_blob(self.format, self.format_quality, self.format_speed)
					.unwrap(),
			);
			self.compression = TileCompression::Uncompressed;
		}
	}
	fn materialize_content(&mut self) {
		if self.content.is_none() {
			assert!(self.blob.is_some());
			self.decompress_blob();
			self.content = Some(TileContent::from_blob(self.blob.as_ref().unwrap(), self.format).unwrap());
		}
	}

	pub fn as_blob(&mut self, compression: TileCompression) -> &Blob {
		self.materialize_blob();
		self.recompress_blob(compression);
		self.blob.as_ref().unwrap()
	}
	fn as_content(&mut self) -> &TileContent {
		self.materialize_content();
		self.content.as_ref().unwrap()
	}
	pub fn as_image(&mut self) -> &DynamicImage {
		match self.as_content() {
			TileContent::Raster(image) => image,
			_ => panic!("Tile does not contain raster image"),
		}
	}
	pub fn as_vector(&mut self) -> &VectorTile {
		match self.as_content() {
			TileContent::Vector(vector_tile) => vector_tile,
			_ => panic!("Tile does not contain vector data"),
		}
	}

	pub fn into_blob(mut self, compression: TileCompression) -> Blob {
		self.materialize_blob();
		self.recompress_blob(compression);
		self.blob.unwrap()
	}
	fn into_content(mut self) -> TileContent {
		self.materialize_content();
		self.content.unwrap()
	}
	pub fn into_image(self) -> DynamicImage {
		match self.into_content() {
			TileContent::Raster(image) => image,
			_ => panic!("Tile does not contain raster image"),
		}
	}
	pub fn into_vector(self) -> VectorTile {
		match self.into_content() {
			TileContent::Vector(vector_tile) => vector_tile,
			_ => panic!("Tile does not contain vector data"),
		}
	}

	fn as_content_mut(&mut self) -> &mut TileContent {
		self.materialize_content();
		self.delete_blob();
		self.content.as_mut().unwrap()
	}
	pub fn as_image_mut(&mut self) -> &mut DynamicImage {
		match self.as_content_mut() {
			TileContent::Raster(image) => image,
			_ => panic!("Tile does not contain raster image"),
		}
	}
	pub fn as_vector_mut(&mut self) -> &mut VectorTile {
		match self.as_content_mut() {
			TileContent::Vector(vector_tile) => vector_tile,
			_ => panic!("Tile does not contain vector data"),
		}
	}

	pub fn format(&self) -> TileFormat {
		self.format
	}
	pub fn compression(&self) -> TileCompression {
		self.compression
	}

	pub fn change_format(&mut self, format: TileFormat, quality: Option<u8>, speed: Option<u8>) {
		self.materialize_content();
		self.delete_blob();
		self.compression = TileCompression::Uncompressed;
		self.format = format;
		if quality.is_some() {
			self.format_quality = quality;
		}
		if speed.is_some() {
			self.format_speed = speed;
		}
	}

	pub fn change_compression(&mut self, compression: TileCompression) {
		if self.blob.is_some() {
			self.recompress_blob(compression);
		} else {
			self.compression = compression;
		}
	}

	pub fn has_blob(&self) -> bool {
		self.blob.is_some()
	}
	pub fn has_content(&self) -> bool {
		self.content.is_some()
	}
}

impl Debug for Tile {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Tile")
			.field("has_blob", &self.has_blob())
			.field("has_content", &self.has_content())
			.field("format", &self.format)
			.field("compression", &self.compression)
			.finish()
	}
}

impl CacheValue for Tile {
	fn write_to_cache(&self, writer: &mut Vec<u8>) -> anyhow::Result<()> {
		todo!()
	}

	fn read_from_cache(reader: &mut std::io::Cursor<&[u8]>) -> anyhow::Result<Self> {
		todo!()
	}
}
