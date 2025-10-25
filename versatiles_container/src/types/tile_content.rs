use anyhow::{Result, bail};
use versatiles_core::{Blob, TileFormat, TileType};
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::{DynamicImage, DynamicImageTraitConvert};

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

	pub fn from_image(image: DynamicImage, format: TileFormat) -> Self {
		assert!(format.to_type().is_raster());
		TileContent::Raster(image)
	}

	pub fn from_vector(vector: VectorTile, format: TileFormat) -> Self {
		assert!(format.to_type().is_vector());
		TileContent::Vector(vector)
	}
}
