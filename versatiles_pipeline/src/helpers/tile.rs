use anyhow::{Ok, Result, anyhow, bail, ensure};
use versatiles_core::{
	Blob, TileCompression, TileFormat, TileType,
	utils::{compress, decompress_ref, recompress},
};
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::{DynamicImage, DynamicImageTraitConvert, encode};

#[derive(Clone)]
pub struct Tile {
	compression: TileCompression,
	format: TileFormat,
	blob: Option<Blob>,
	image: Option<DynamicImage>,
	vector: Option<VectorTile>,
}

impl Tile {
	fn new(format: TileFormat, compression: TileCompression) -> Self {
		Self {
			compression,
			format,
			blob: None,
			image: None,
			vector: None,
		}
	}

	pub fn from_blob(blob: Blob, format: TileFormat, compression: TileCompression) -> Self {
		let mut tile = Self::new(format, compression);
		tile.blob = Some(blob);
		tile
	}

	pub fn from_image(image: DynamicImage, format: TileFormat, compression: TileCompression) -> Self {
		let mut tile = Self::new(format, compression);
		tile.image = Some(image);
		tile
	}

	pub fn from_vector(vector: VectorTile, format: TileFormat, compression: TileCompression) -> Self {
		let mut tile = Self::new(format, compression);
		tile.vector = Some(vector);
		tile
	}

	pub fn ensure_blob(&mut self) -> Result<()> {
		if self.blob.is_none() {
			let blob = match self.format.get_type() {
				TileType::Raster => {
					let image = self.image.as_ref().ok_or(anyhow!("tile has no image data"))?;
					encode(image, self.format, None, None)?
				}
				TileType::Vector => {
					let vector = self.vector.as_ref().ok_or(anyhow!("tile has no vector data"))?;
					vector.to_blob()?
				}
				_ => return Err(anyhow!("unknown tile type")),
			};
			self.blob = Some(compress(blob, &self.compression)?);
		}
		Ok(())
	}

	pub fn ensure_image(&mut self) -> Result<()> {
		ensure!(self.format.get_type().is_raster(), "tile is not raster data");
		if self.image.is_none() {
			ensure!(self.blob.is_some(), "tile has no data");
			let format = self.format;
			let blob = self.blob.as_ref().ok_or(anyhow!("tile has no blob data"))?;
			self.image = Some(if self.compression == TileCompression::Uncompressed {
				DynamicImage::from_blob(blob, format)?
			} else {
				DynamicImage::from_blob(&decompress_ref(blob, &self.compression)?, format)?
			});
		}
		Ok(())
	}

	pub fn ensure_vector(&mut self) -> Result<()> {
		ensure!(self.format.get_type().is_vector(), "tile is not vector data");
		if self.vector.is_none() {
			ensure!(self.blob.is_some(), "tile has no data");
			let blob = self.blob.as_ref().ok_or(anyhow!("tile has no blob data"))?;
			self.vector = Some(if self.compression == TileCompression::Uncompressed {
				VectorTile::from_blob(blob)?
			} else {
				VectorTile::from_blob(&decompress_ref(blob, &self.compression)?)?
			});
		}
		Ok(())
	}

	pub fn blob(&mut self) -> Result<&Blob> {
		self.ensure_blob()?;
		Ok(self.blob.as_ref().ok_or(anyhow!("tile has no blob data"))?)
	}

	pub fn image(&mut self) -> Result<&DynamicImage> {
		self.ensure_image()?;
		Ok(self.image.as_ref().ok_or(anyhow!("tile has no image data"))?)
	}

	pub fn vector(&mut self) -> Result<&VectorTile> {
		self.ensure_vector()?;
		Ok(self.vector.as_ref().ok_or(anyhow!("tile has no vector data"))?)
	}

	pub fn blob_mut(&mut self) -> Result<&mut Blob> {
		self.ensure_blob()?;
		self.image = None;
		self.vector = None;
		Ok(self.blob.as_mut().ok_or(anyhow!("tile has no blob data"))?)
	}

	pub fn image_mut(&mut self) -> Result<&mut DynamicImage> {
		self.ensure_image()?;
		self.blob = None;
		Ok(self.image.as_mut().ok_or(anyhow!("tile has no image data"))?)
	}

	pub fn vector_mut(&mut self) -> Result<&mut VectorTile> {
		self.ensure_vector()?;
		self.blob = None;
		Ok(self.vector.as_mut().ok_or(anyhow!("tile has no vector data"))?)
	}

	pub fn into_blob(mut self) -> Result<Blob> {
		self.ensure_blob()?;
		Ok(self.blob.take().ok_or(anyhow!("tile has no blob data"))?)
	}

	pub fn into_image(mut self) -> Result<DynamicImage> {
		self.ensure_image()?;
		Ok(self.image.take().ok_or(anyhow!("tile has no image data"))?)
	}

	pub fn into_vector(mut self) -> Result<VectorTile> {
		self.ensure_vector()?;
		Ok(self.vector.take().ok_or(anyhow!("tile has no vector data"))?)
	}

	pub fn map_image<F>(mut self, f: F) -> Result<Self>
	where
		F: FnOnce(DynamicImage) -> Result<DynamicImage>,
	{
		self.ensure_image()?;
		let image = self.image.take().ok_or(anyhow!("tile has no image data"))?;
		self.image = Some(f(image)?);
		self.blob = None;
		Ok(self)
	}

	pub fn encode_raster(
		&mut self,
		format: Option<TileFormat>,
		compression: Option<TileCompression>,
		quality: Option<u8>,
		speed: Option<u8>,
	) -> Result<()> {
		self.ensure_image()?;

		self.format = format.unwrap_or(self.format);
		self.compression = compression.unwrap_or(self.compression);

		let blob = encode(
			self.image.as_ref().ok_or(anyhow!("tile has no image data"))?,
			self.format,
			quality,
			speed,
		)?;

		self.blob = Some(compress(blob, &self.compression)?);
		Ok(())
	}

	pub fn filter_map_vector<F>(mut self, f: F) -> Result<Option<Self>>
	where
		F: FnOnce(VectorTile) -> Result<Option<VectorTile>>,
	{
		self.ensure_vector()?;
		let vector = self.vector.take().ok_or(anyhow!("tile has no vector data"))?;
		if let Some(vector) = f(vector)? {
			self.vector = Some(vector);
			self.blob = None;
			Ok(Some(self))
		} else {
			Ok(None)
		}
	}

	pub fn change_format(&mut self, format: TileFormat) -> Result<()> {
		if self.format == format {
			return Ok(());
		}
		ensure!(
			self.format.get_type() == format.get_type(),
			"cannot change tile type from {} to {}",
			self.format,
			format
		);
		match self.format.get_type() {
			TileType::Raster => self.ensure_image()?,
			TileType::Vector => self.ensure_vector()?,
			_ => bail!("unknown tile type"),
		}
		self.format = format;
		self.blob = None;
		Ok(())
	}

	pub fn change_compression(&mut self, compression: TileCompression) -> Result<()> {
		if self.compression == compression {
			return Ok(());
		}
		if let Some(blob) = self.blob.as_mut() {
			*blob = recompress(std::mem::take(blob), &self.compression, &compression)?;
		}
		self.compression = compression;
		Ok(())
	}
}
