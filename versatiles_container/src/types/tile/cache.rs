//! CacheValue implementation for serializing Tile to and from cache.

use super::Tile;
use crate::{CacheValue, TileContent};
use anyhow::Result;
use std::io::Cursor;
use versatiles_core::{Blob, TileCompression, TileFormat};
use versatiles_derive::context;

impl CacheValue for Tile {
	#[context("serializing Tile to cache (has_blob={}, has_content={})", self.has_blob(), self.has_content())]
	fn write_to_cache(&self, writer: &mut Vec<u8>) -> Result<()> {
		self.blob.write_to_cache(writer)?;
		self.content.write_to_cache(writer)?;
		self.format.write_to_cache(writer)?;
		self.compression.write_to_cache(writer)?;
		self.format_quality.write_to_cache(writer)?;
		self.format_speed.write_to_cache(writer)?;
		Ok(())
	}

	#[context("deserializing Tile from cache")]
	fn read_from_cache(reader: &mut Cursor<&[u8]>) -> Result<Self> {
		let blob = Option::<Blob>::read_from_cache(reader)?;
		let content = Option::<TileContent>::read_from_cache(reader)?;
		let format = TileFormat::read_from_cache(reader)?;
		let compression = TileCompression::read_from_cache(reader)?;
		let format_quality = Option::<u8>::read_from_cache(reader)?;
		let format_speed = Option::<u8>::read_from_cache(reader)?;
		Ok(Tile {
			blob,
			content,
			format,
			compression,
			format_quality,
			format_speed,
		})
	}
}
