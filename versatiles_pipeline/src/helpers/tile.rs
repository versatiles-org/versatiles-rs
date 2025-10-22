//! Tile helper for VersaTiles pipeline
//!
//! This module defines [`Tile`], a small container that can hold a tile as
//! either a compressed/raw binary blob, a decoded raster image, or a decoded
//! vector-tile structure. Only one concrete representation needs to be
//! materialized at a time. Conversions are performed lazily on demand and the
//! other cached representations are invalidated automatically to keep memory
//! usage predictable.
//!
//! **Key ideas**
//! - *Format vs. Type:* [`TileFormat`] encodes the concrete encoding (e.g. PNG,
//!   WEBP, MVT). Each format has a [`TileType`] of either raster or vector.
//! - *Compression:* The optional outer container [`TileCompression`] (e.g.
//!   gzip, brotli) is applied to the blob representation only. Decoded
//!   [`DynamicImage`] / [`VectorTile`] are always uncompressed in memory.
//! - *Lazy materialization:* Accessors like [`Tile::image`],
//!   [`Tile::vector`], or [`Tile::blob`] decode/encode only when needed.
//! - *Cache invalidation:* Any mutating access (`*_mut`) or mapping
//!   operation (`map_*`) invalidates other representations to prevent stale
//!   data.
//!
//! All functions return [`anyhow::Result`] with descriptive error messages on
//! format/typing mismatches or missing data.
use anyhow::{Ok, Result, anyhow, bail, ensure};
use versatiles_core::{
	Blob, TileCompression, TileFormat, TileType,
	utils::{compress, decompress_ref, recompress},
};
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::{DynamicImage, DynamicImageTraitConvert, encode};

/// A lazily materialized map tile.
///
/// A `Tile` remembers its logical [`TileFormat`] and optional outer
/// [`TileCompression`]. Internally it can store one of three representations:
///
/// - `blob`: encoded bytes, possibly additionally compressed
/// - `image`: decoded raster [`DynamicImage`]
/// - `vector`: decoded [`VectorTile`]
///
/// At most one representation needs to exist at any time. Accessors will
/// materialize the requested representation on demand and drop/clear other
/// representations as needed to avoid inconsistencies.
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

	/// Construct a tile from an already encoded `blob`.
	///
	/// The `format` must match the content inside the blob and `compression`
	/// denotes the outer container (e.g. gzip). No decoding is performed here;
	/// conversion happens lazily on first access.
	pub fn from_blob(blob: Blob, format: TileFormat, compression: TileCompression) -> Self {
		let mut tile = Self::new(format, compression);
		tile.blob = Some(blob);
		tile
	}

	/// Construct a raster tile from a decoded [`DynamicImage`].
	///
	/// The `format` must be a raster format (e.g. PNG/WEBP). The image is kept
	/// as-is until a blob is requested, at which point it will be encoded and
	/// optionally compressed.
	pub fn from_image(image: DynamicImage, format: TileFormat, compression: TileCompression) -> Self {
		assert_eq!(format.get_type(), TileType::Raster);
		let mut tile = Self::new(format, compression);
		tile.image = Some(image);
		tile
	}

	/// Construct a vector tile from a decoded [`VectorTile`].
	///
	/// The `format` must be a vector format (e.g. MVT). The vector data is kept
	/// as-is until a blob is requested.
	pub fn from_vector(vector: VectorTile, format: TileFormat, compression: TileCompression) -> Self {
		assert_eq!(format.get_type(), TileType::Vector);
		let mut tile = Self::new(format, compression);
		tile.vector = Some(vector);
		tile
	}

	fn materialize_blob(&mut self) -> Result<()> {
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

	fn materialize_image(&mut self) -> Result<()> {
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

	fn materialize_vector(&mut self) -> Result<()> {
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

	/// Ensure and return the encoded blob representation.
	///
	/// If only an image/vector exists, it will be (re)encoded using the current
	/// [`TileFormat`] and then compressed with the current [`TileCompression`].
	pub fn blob(&mut self) -> Result<&Blob> {
		self.materialize_blob()?;
		Ok(self.blob.as_ref().ok_or(anyhow!("tile has no blob data"))?)
	}

	/// Ensure and return the decoded raster image.
	///
	/// Fails if this tile is not a raster tile. If necessary, the blob will be
	/// decompressed and decoded.
	pub fn image(&mut self) -> Result<&DynamicImage> {
		self.materialize_image()?;
		Ok(self.image.as_ref().ok_or(anyhow!("tile has no image data"))?)
	}

	/// Ensure and return the decoded vector tile.
	///
	/// Fails if this tile is not a vector tile. If necessary, the blob will be
	/// decompressed and decoded.
	pub fn vector(&mut self) -> Result<&VectorTile> {
		self.materialize_vector()?;
		Ok(self.vector.as_ref().ok_or(anyhow!("tile has no vector data"))?)
	}

	/// Get a mutable reference to the blob, materializing it if needed.
	///
	/// Invalidates any cached decoded image/vector representation to avoid
	/// inconsistencies.
	pub fn blob_mut(&mut self) -> Result<&mut Blob> {
		self.materialize_blob()?;
		self.image = None;
		self.vector = None;
		Ok(self.blob.as_mut().ok_or(anyhow!("tile has no blob data"))?)
	}

	/// Get a mutable reference to the decoded raster image, materializing it if needed.
	///
	/// Clears any cached blob because subsequent encoding must reflect the
	/// mutation.
	pub fn image_mut(&mut self) -> Result<&mut DynamicImage> {
		self.materialize_image()?;
		self.blob = None;
		Ok(self.image.as_mut().ok_or(anyhow!("tile has no image data"))?)
	}

	/// Get a mutable reference to the decoded vector tile, materializing it if needed.
	///
	/// Clears any cached blob because subsequent encoding must reflect the
	/// mutation.
	pub fn vector_mut(&mut self) -> Result<&mut VectorTile> {
		self.materialize_vector()?;
		self.blob = None;
		Ok(self.vector.as_mut().ok_or(anyhow!("tile has no vector data"))?)
	}

	/// Consume the tile and return its encoded blob.
	///
	/// Materializes the blob if necessary.
	pub fn into_blob(mut self) -> Result<Blob> {
		self.materialize_blob()?;
		Ok(self.blob.take().ok_or(anyhow!("tile has no blob data"))?)
	}

	/// Consume the tile and return its decoded raster image.
	///
	/// Fails if the tile is not a raster tile.
	pub fn into_image(mut self) -> Result<DynamicImage> {
		self.materialize_image()?;
		Ok(self.image.take().ok_or(anyhow!("tile has no image data"))?)
	}

	/// Consume the tile and return its decoded vector tile.
	///
	/// Fails if the tile is not a vector tile.
	pub fn into_vector(mut self) -> Result<VectorTile> {
		self.materialize_vector()?;
		Ok(self.vector.take().ok_or(anyhow!("tile has no vector data"))?)
	}

	/// Apply a fallible transformation to the decoded raster image.
	///
	/// Lazily materializes the image, passes ownership to `f`, stores the result,
	/// and invalidates the blob cache. Returns the updated `Tile`.
	pub fn map_image<F>(mut self, f: F) -> Result<Self>
	where
		F: FnOnce(DynamicImage) -> Result<DynamicImage>,
	{
		self.materialize_image()?;
		let image = self.image.take().ok_or(anyhow!("tile has no image data"))?;
		self.image = Some(f(image)?);
		self.blob = None;
		Ok(self)
	}

	/// Apply a fallible transformation to the decoded vector tile.
	///
	/// Lazily materializes the vector, passes ownership to `f`, stores the
	/// result, and invalidates the blob cache. Returns the updated `Tile`.
	pub fn map_vector<F>(mut self, f: F) -> Result<Self>
	where
		F: FnOnce(VectorTile) -> Result<VectorTile>,
	{
		self.materialize_vector()?;
		let vector = self.vector.take().ok_or(anyhow!("tile has no vector data"))?;
		self.vector = Some(f(vector)?);
		self.blob = None;
		Ok(self)
	}

	/// Re-encode the raster image into a new format/compression.
	///
	/// Any `None` argument keeps the current setting. `quality` and `speed`
	/// are passed to the encoder when supported by the chosen [`TileFormat`].
	/// Materializes the image, encodes, and stores the resulting compressed blob.
	pub fn reencode_raster(
		&mut self,
		format: Option<TileFormat>,
		compression: Option<TileCompression>,
		quality: Option<u8>,
		speed: Option<u8>,
	) -> Result<()> {
		self.materialize_image()?;

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

	/// Transform the vector tile and optionally discard it.
	///
	/// Calls `f` with the decoded vector. If `f` returns `Ok(None)`, the tile is
	/// dropped and `Ok(None)` is returned. If a vector is returned, it replaces
	/// the previous value and the blob cache is cleared.
	pub fn filter_map_vector<F>(mut self, f: F) -> Result<Option<Self>>
	where
		F: FnOnce(VectorTile) -> Result<Option<VectorTile>>,
	{
		self.materialize_vector()?;
		let vector = self.vector.take().ok_or(anyhow!("tile has no vector data"))?;
		if let Some(vector) = f(vector)? {
			self.vector = Some(vector);
			self.blob = None;
			Ok(Some(self))
		} else {
			Ok(None)
		}
	}

	/// Change the tile's *format* while preserving its *type* (raster/vector).
	///
	/// Materializes the decoded representation first to avoid round‑tripping via
	/// the old encoding. Returns an error if the requested format has a different
	/// [`TileType`] than the current one.
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
			TileType::Raster => self.materialize_image()?,
			TileType::Vector => self.materialize_vector()?,
			_ => bail!("unknown tile type"),
		}
		self.format = format;
		self.blob = None;
		Ok(())
	}

	/// Change the outer blob compression in-place.
	///
	/// If a blob exists, it is recompressed without touching decoded data.
	/// Otherwise only the setting is updated for the next encoding step.
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

	/// Return the current on-disk [`TileFormat`].
	pub fn format(&self) -> TileFormat {
		self.format
	}

	/// Return the current outer [`TileCompression`].
	pub fn compression(&self) -> TileCompression {
		self.compression
	}

	/// Whether an encoded blob is currently cached.
	pub fn has_blob(&self) -> bool {
		self.blob.is_some()
	}

	/// Whether a decoded raster image is currently cached.
	pub fn has_image(&self) -> bool {
		self.image.is_some()
	}

	/// Whether a decoded vector tile is currently cached.
	pub fn has_vector(&self) -> bool {
		self.vector.is_some()
	}
}

impl std::fmt::Debug for Tile {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Tile")
			.field("compression", &self.compression)
			.field("format", &self.format)
			.field(
				"blob",
				&self
					.blob
					.as_ref()
					.map_or(String::from("none"), |b| format!("{}", b.len())),
			)
			.field(
				"image",
				&self
					.image
					.as_ref()
					.map_or(String::from("none"), |i| format!("{}x{}", i.width(), i.height())),
			)
			.field(
				"vector",
				&self
					.vector
					.as_ref()
					.map_or(String::from("none"), |v| format!("{} layers", v.layers.len())),
			)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use imageproc::image::{self, GenericImage, RgbaImage};
	use rstest::rstest;

	fn tiny_rgba(r: u8, g: u8, b: u8, a: u8) -> DynamicImage {
		let mut img = RgbaImage::new(1, 1);
		img.put_pixel(0, 0, image::Rgba([r, g, b, a]));
		DynamicImage::from(img)
	}

	#[rstest]
	fn raster_roundtrip_blob_encode_decode() {
		let img = tiny_rgba(10, 20, 30, 255);
		let mut t = Tile::from_image(img.clone(), TileFormat::PNG, TileCompression::Gzip);

		// materialize blob
		let blob_len_1 = t.blob().unwrap().len();
		assert!(blob_len_1 > 0);

		// drop caches and re-decode from blob
		let blob = t.into_blob().unwrap();
		let mut t2 = Tile::from_blob(blob, TileFormat::PNG, TileCompression::Gzip);
		let im2 = t2.image().unwrap();
		assert_eq!(im2.width(), 1);
		assert_eq!(im2.height(), 1);
	}

	#[rstest]
	fn change_compression_recompresses_in_place() {
		let img = tiny_rgba(1, 2, 3, 255);
		let mut t = Tile::from_image(img, TileFormat::WEBP, TileCompression::Gzip);
		let before = t.blob().unwrap().len();
		t.change_compression(TileCompression::Brotli).unwrap();
		let after = t.blob().unwrap().len();
		assert_ne!(before, after, "blob should have been recompressed");
	}

	#[rstest]
	fn change_format_same_type_allowed() {
		let img = tiny_rgba(0, 0, 0, 255);
		let mut t = Tile::from_image(img, TileFormat::PNG, TileCompression::Uncompressed);
		t.change_format(TileFormat::WEBP).unwrap(); // still raster
		assert_eq!(t.format().get_type(), TileType::Raster);
	}

	#[rstest]
	fn change_format_cross_type_errors() {
		let img = tiny_rgba(0, 0, 0, 255);
		let mut t = Tile::from_image(img, TileFormat::PNG, TileCompression::Uncompressed);
		let err = t.change_format(TileFormat::MVT).unwrap_err();
		let s = err.to_string();
		assert!(s.contains("cannot change tile type"));
	}

	#[rstest]
	fn map_image_invalidates_blob() {
		let img = tiny_rgba(10, 10, 10, 255);
		let mut t = Tile::from_image(img, TileFormat::PNG, TileCompression::Gzip);
		// ensure blob exists
		assert!(t.has_blob() || t.blob().is_ok());

		// mutate the image
		let t = t
			.map_image(|mut im| {
				// turn pixel white
				im.put_pixel(0, 0, image::Rgba([255, 255, 255, 255]));
				Ok(im)
			})
			.unwrap();

		assert!(t.has_image());
		assert!(!t.has_blob(), "blob must be invalidated after image map");
	}

	#[rstest]
	fn reencode_raster_produces_blob() {
		let img = tiny_rgba(33, 44, 55, 255);
		let mut t = Tile::from_image(img, TileFormat::WEBP, TileCompression::Uncompressed);
		t.reencode_raster(Some(TileFormat::WEBP), Some(TileCompression::Brotli), Some(75), Some(4))
			.unwrap();
		assert!(t.has_blob());
		assert_eq!(t.format(), TileFormat::WEBP);
		assert_eq!(t.compression(), TileCompression::Brotli);
	}
}
