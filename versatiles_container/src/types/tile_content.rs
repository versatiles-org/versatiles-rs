use anyhow::{Result, bail};
use byteorder::{ReadBytesExt, WriteBytesExt};
use std::io::Cursor;
use versatiles_core::{Blob, TileFormat, TileType};
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::{DynamicImage, DynamicImageTraitConvert};

use crate::CacheValue;

#[derive(Clone, PartialEq)]
pub enum TileContent {
	// Placeholder for different tile content types, e.g., Image, Vector, etc.
	Raster(DynamicImage),
	Vector(VectorTile),
}

impl TileContent {
	pub fn to_blob(&self, format: TileFormat, quality: Option<u8>, speed: Option<u8>) -> Result<Blob> {
		match self {
			TileContent::Raster(image) => image.to_blob(format, quality, speed),
			TileContent::Vector(vector) => vector.to_blob(),
		}
	}

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

	pub fn from_image(image: DynamicImage) -> Self {
		TileContent::Raster(image)
	}

	pub fn from_vector(vector: VectorTile) -> Self {
		TileContent::Vector(vector)
	}

	pub fn as_image(&self) -> Result<&DynamicImage> {
		match self {
			TileContent::Raster(image) => Ok(&image),
			_ => bail!("Tile does not contain raster image"),
		}
	}

	pub fn as_vector(&self) -> Result<&VectorTile> {
		match self {
			TileContent::Vector(vector_tile) => Ok(&vector_tile),
			_ => bail!("Tile does not contain vector data"),
		}
	}

	pub fn as_image_mut(&mut self) -> Result<&mut DynamicImage> {
		match self {
			TileContent::Raster(image) => Ok(image),
			_ => bail!("Tile does not contain raster image"),
		}
	}

	pub fn as_vector_mut(&mut self) -> Result<&mut VectorTile> {
		match self {
			TileContent::Vector(vector_tile) => Ok(vector_tile),
			_ => bail!("Tile does not contain vector data"),
		}
	}

	pub fn into_image(self) -> Result<DynamicImage> {
		match self {
			TileContent::Raster(image) => Ok(image),
			_ => bail!("Tile does not contain raster image"),
		}
	}

	pub fn into_vector(self) -> Result<VectorTile> {
		match self {
			TileContent::Vector(vector_tile) => Ok(vector_tile),
			_ => bail!("Tile does not contain vector data"),
		}
	}
}

impl CacheValue for TileContent {
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
