//! `TileContent` holds the decoded, in-memory representation of a map tile.
//!
//! It is either:
//! - `Raster(DynamicImage)` for raster tiles, or
//! - `Vector(VectorTile)` for vector tiles.
//!
//! This enum is used internally by higher-level types (like `Tile`) to lazily convert
//! between an encoded blob and a decoded content representation. Expensive conversions
//! are wrapped with contextual error messages via the `#[context(...)]` attribute.

use crate::CacheValue;
use anyhow::{Result, bail};
use byteorder::{ReadBytesExt, WriteBytesExt};
use std::io::Cursor;
use versatiles_core::{Blob, TileFormat, TileType};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::{DynamicImage, DynamicImageTraitConvert};

/// Decoded tile content (raster or vector).
///
/// `TileContent` represents the decoded form of a tile used for on-the-fly processing.
///
/// # Examples
/// Converting content to a blob:
/// ```no_run
/// use versatiles_container::TileContent;
/// use versatiles_core::TileFormat::PNG;
/// # let img = versatiles_image::DynamicImage::new_rgb8(1,1);
/// let content = TileContent::from_image(img);
/// let blob = content.to_blob(PNG, None, None).expect("encode");
/// assert!(!blob.is_empty());
/// ```
#[derive(Clone, PartialEq)]
pub enum TileContent {
	/// Raster tile content stored as a `DynamicImage`.
	Raster(DynamicImage),
	/// Vector tile content stored as a `VectorTile`.
	Vector(VectorTile),
}

impl TileContent {
	/// Encode this content into a blob in the given `format`.
	///
	/// For raster content, optional `quality` and `speed` hints are honored when supported by the encoder.
	/// Vector content ignores `quality`/`speed` and uses its native encoder.
	#[context("converting tile to blob: format={:?}, q={:?}, s={:?}", format, quality, speed)]
	pub fn to_blob(&self, format: TileFormat, quality: Option<u8>, speed: Option<u8>) -> Result<Blob> {
		match self {
			TileContent::Raster(image) => image.to_blob(format, quality, speed),
			TileContent::Vector(vector) => vector.to_blob(),
		}
	}

	/// Decode a `Blob` into `TileContent` using the provided tile `format`.
	///
	/// The `format` determines whether the blob is interpreted as raster or vector.
	/// Returns an error for unsupported or mismatched formats.
	#[context("decoding tile from blob ({} bytes) as {:?}", blob.len(), format)]
	pub fn from_blob(blob: &Blob, format: TileFormat) -> Result<Self> {
		Ok(match format.to_type() {
			TileType::Raster => {
				let image = DynamicImage::from_blob(blob, format)?;
				TileContent::Raster(image)
			}
			TileType::Vector => {
				let vector = VectorTile::from_blob(blob)?;
				TileContent::Vector(vector)
			}
			_ => bail!("Unsupported tile format for decoding: {format:?}"),
		})
	}

	/// Construct raster content from a `DynamicImage`.
	pub fn from_image(image: DynamicImage) -> Self {
		TileContent::Raster(image)
	}

	/// Construct vector content from a `VectorTile`.
	pub fn from_vector(vector: VectorTile) -> Self {
		TileContent::Vector(vector)
	}

	/// Borrow the raster image; fails if this is vector content.
	#[context("accessing raster image from tile content")]
	pub fn as_image(&self) -> Result<&DynamicImage> {
		match self {
			TileContent::Raster(image) => Ok(image),
			_ => bail!("Tile does not contain raster image"),
		}
	}

	/// Borrow the vector data; fails if this is raster content.
	#[context("accessing vector data from tile content")]
	pub fn as_vector(&self) -> Result<&VectorTile> {
		match self {
			TileContent::Vector(vector_tile) => Ok(vector_tile),
			_ => bail!("Tile does not contain vector data"),
		}
	}

	/// Mutably borrow the raster image; fails if this is vector content.
	pub fn as_image_mut(&mut self) -> Result<&mut DynamicImage> {
		match self {
			TileContent::Raster(image) => Ok(image),
			_ => bail!("Tile does not contain raster image"),
		}
	}

	/// Mutably borrow the vector data; fails if this is raster content.
	pub fn as_vector_mut(&mut self) -> Result<&mut VectorTile> {
		match self {
			TileContent::Vector(vector_tile) => Ok(vector_tile),
			_ => bail!("Tile does not contain vector data"),
		}
	}

	/// Consume and return the raster image; fails if this is vector content.
	#[context("Failed converting TileContent into image")]
	pub fn into_image(self) -> Result<DynamicImage> {
		match self {
			TileContent::Raster(image) => Ok(image),
			_ => bail!("Tile does not contain raster image"),
		}
	}

	/// Consume and return the vector data; fails if this is raster content.
	#[context("Failed converting TileContent into vector")]
	pub fn into_vector(self) -> Result<VectorTile> {
		match self {
			TileContent::Vector(vector_tile) => Ok(vector_tile),
			_ => bail!("Tile does not contain vector data"),
		}
	}
}

/// Binary cache (de)serialization for `TileContent`.
///
/// A leading one-byte tag distinguishes variants:
/// - `0` = `Raster(DynamicImage)`
/// - `1` = `Vector(VectorTile)`
impl CacheValue for TileContent {
	/// Write a compact binary representation of the content to `writer`.
	#[context("serializing tile to cache buffer (pre-len={})", writer.len())]
	fn write_to_cache(&self, writer: &mut Vec<u8>) -> Result<()> {
		match self {
			TileContent::Raster(image) => {
				writer.write_u8(0)?; // Type identifier for Raster
				image.write_to_cache(writer)
			}
			TileContent::Vector(vector) => {
				writer.write_u8(1)?; // Type identifier for Vector
				vector.to_blob()?.write_to_cache(writer)
			}
		}
	}

	/// Read content from the compact binary representation produced by `write_to_cache`.
	#[context("deserializing tile from cache buffer")]
	fn read_from_cache(reader: &mut Cursor<&[u8]>) -> Result<Self> {
		let content_type = reader.read_u8()?;
		match content_type {
			0 => {
				let image = DynamicImage::read_from_cache(reader)?;
				Ok(TileContent::Raster(image))
			}
			1 => {
				let vector = VectorTile::from_blob(&Blob::read_from_cache(reader)?)?;
				Ok(TileContent::Vector(vector))
			}
			_ => bail!("Unknown TileContent type identifier: {content_type}"),
		}
	}
}
