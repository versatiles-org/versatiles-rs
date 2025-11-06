//! `Tile` is a small, lazy container for raster/vector map tiles.
//!
//! It can hold either:
//! - a raw encoded **blob** (optionally compressed), or
//! - a decoded **content** representation (raster `DynamicImage` or vector `VectorTile`),
//!   and transparently materializes the other representation on demand.
//!
//! The type keeps track of the tile **format** (e.g. `PNG`, `MVT`) and the transport
//! **compression** (e.g. `Gzip`, `Uncompressed`). Quality and speed hints are stored
//! for formats that support them and are applied when (re-)encoding.
//!
//! All expensive conversions are performed lazily and are wrapped with contextual error
//! messages via the `#[context(...)]` attribute from `versatiles_derive`.
//!
//! Typical usage:
//! - Build from an image/vector and retrieve a blob ready to send over the wire.
//! - Build from a blob received from storage, then inspect or mutate the decoded content.

use anyhow::{Result, anyhow, ensure};
use std::{fmt::Debug, io::Cursor};
use versatiles_core::{
	Blob, TileCompression, TileFormat,
	utils::{decompress_ref, recompress},
};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::DynamicImage;

use crate::{CacheValue, TileContent};

/// A lazy tile container that can hold either an encoded blob or decoded content.
///
/// The `Tile` ensures only the necessary representation is kept in memory:
/// when you request a blob, content is encoded on demand; when you request content,
/// a present blob is decoded on demand. Changing the content invalidates the blob,
/// but format/compression flags are preserved until re-materialization.
///
/// # Examples
/// Creating a raster tile and getting an uncompressed blob:
/// ```no_run
/// use versatiles_container::Tile;
/// use versatiles_core::{TileCompression::Uncompressed, TileFormat::PNG};
/// # let img = versatiles_image::DynamicImage::new_rgb8(1,1);
/// let mut tile = Tile::from_image(img, PNG).expect("raster tile");
/// let blob = tile.as_blob(Uncompressed).expect("to blob");
/// assert!(!blob.is_empty());
/// ```
///
/// Creating a vector tile and accessing its decoded data:
/// ```no_run
/// use versatiles_container::Tile;
/// use versatiles_core::TileFormat::MVT;
/// let vt = versatiles_geometry::vector_tile::VectorTile::default();
/// let mut tile = Tile::from_vector(vt, MVT).expect("vector tile");
/// let _vref = tile.as_vector().expect("decoded vector");
/// ```
#[derive(Clone, PartialEq)]
pub struct Tile {
	blob: Option<Blob>,
	content: Option<TileContent>,
	format: TileFormat,
	compression: TileCompression,
	format_quality: Option<u8>,
	format_speed: Option<u8>,
}

/// Constructors and lazy accessors for `Tile`.
impl Tile {
	/// Construct a `Tile` from an already encoded `blob`.
	///
	/// The provided `compression` describes the outer transport compression of the blob
	/// (e.g., `Gzip` or `Uncompressed`), and `format` describes the inner tile format
	/// (e.g., `PNG`, `WEBP`, `MVT`).
	///
	/// This does **not** decode the blob. Decoding happens lazily when content is requested.
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

	#[context("creating raster tile (format={:?})", format)]
	/// Construct a raster `Tile` from a `DynamicImage`.
	///
	/// The `format` must be a raster format (e.g., `PNG`, `WEBP`). The tile starts with
	/// decoded content and no blob; a blob is created on first call to `as_blob`/`into_blob`.
	pub fn from_image(image: DynamicImage, format: TileFormat) -> Result<Self> {
		ensure!(format.to_type().is_raster());
		Ok(Self::from_content(TileContent::from_image(image), format))
	}

	#[context("creating vector tile (format={:?})", format)]
	/// Construct a vector `Tile` from a `VectorTile`.
	///
	/// The `format` must be a vector format (e.g., `MVT`). The tile starts with decoded content.
	pub fn from_vector(vector_tile: VectorTile, format: TileFormat) -> Result<Self> {
		ensure!(format.to_type().is_vector());
		Ok(Self::from_content(TileContent::from_vector(vector_tile), format))
	}

	#[context("recompressing blob: {:?} -> {:?}", self.compression, compression)]
	fn recompress_blob(&mut self, compression: TileCompression) -> Result<()> {
		assert!(self.blob.is_some());
		if self.compression != compression {
			self.blob = Some(recompress(self.blob.take().unwrap(), self.compression, compression)?);
			self.compression = compression;
		}
		Ok(())
	}

	#[context("decompressing blob ({:?})", self.compression)]
	fn decompress_blob(&mut self) -> Result<()> {
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

	fn delete_blob(&mut self) {
		self.blob = None;
		self.compression = TileCompression::Uncompressed;
	}

	#[context("materializing blob from content (format={:?}, q={:?}, s={:?})", self.format, self.format_quality, self.format_speed)]
	fn materialize_blob(&mut self) -> Result<()> {
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
	fn materialize_content(&mut self) -> Result<()> {
		if self.content.is_none() {
			ensure!(self.blob.is_some(), "Cannot materialize content without blob");
			self.decompress_blob()?;
			self.content = Some(TileContent::from_blob(self.blob.as_ref().unwrap(), self.format)?);
		}
		Ok(())
	}

	#[context("getting blob (target_compression={:?})", compression)]
	/// Get a reference to the encoded blob, re-(de)compressing as needed.
	///
	/// If no blob exists, the current content is encoded according to `self.format`,
	/// using any stored quality/speed hints. If a blob exists with a different
	/// `self.compression`, it is re-compressed to `compression`.
	///
	/// Returns a reference valid until the next mutating call.
	pub fn as_blob(&mut self, compression: TileCompression) -> Result<&Blob> {
		self.materialize_blob()?;
		self.recompress_blob(compression)?;
		self.blob.as_ref().ok_or(anyhow!("blob should be present"))
	}

	#[context("accessing tile content")]
	fn as_content(&mut self) -> Result<&TileContent> {
		self.materialize_content()?;
		self.content.as_ref().ok_or(anyhow!("content should be present"))
	}

	#[context("accessing raster image from tile")]
	/// Borrow the raster image content, decoding the blob on demand.
	///
	/// Fails if the tile is not of a raster format.
	pub fn as_image(&mut self) -> Result<&DynamicImage> {
		self.as_content()?.as_image()
	}

	#[context("accessing vector data from tile")]
	/// Borrow the vector tile content, decoding the blob on demand.
	///
	/// Fails if the tile is not of a vector format.
	pub fn as_vector(&mut self) -> Result<&VectorTile> {
		self.as_content()?.as_vector()
	}

	#[context("converting tile into blob (target_compression={:?})", compression)]
	/// Consume the tile and return an encoded blob with the requested compression.
	///
	/// This materializes content-to-blob if necessary and applies (re-)compression.
	pub fn into_blob(mut self, compression: TileCompression) -> Result<Blob> {
		self.materialize_blob()?;
		self.recompress_blob(compression)?;
		Ok(self.blob.unwrap())
	}

	#[context("converting tile into content")]
	fn into_content(mut self) -> Result<TileContent> {
		self.materialize_content()?;
		Ok(self.content.unwrap())
	}

	#[context("converting tile into raster image")]
	/// Consume the tile and return the owned raster image.
	///
	/// Fails if the tile is not a raster format.
	pub fn into_image(self) -> Result<DynamicImage> {
		self.into_content()?.into_image()
	}

	#[context("converting tile into vector data")]
	/// Consume the tile and return the owned vector data.
	///
	/// Fails if the tile is not a vector format.
	pub fn into_vector(self) -> Result<VectorTile> {
		self.into_content()?.into_vector()
	}

	#[context("accessing mutable tile content (dropping blob)")]
	fn as_content_mut(&mut self) -> Result<&mut TileContent> {
		self.materialize_content()?;
		self.delete_blob();
		Ok(self.content.as_mut().unwrap())
	}

	#[context("accessing mutable raster image (dropping blob)")]
	/// Mutably borrow the raster image content.
	///
	/// Any existing blob is dropped because it would be stale after mutation.
	pub fn as_image_mut(&mut self) -> Result<&mut DynamicImage> {
		self.as_content_mut()?.as_image_mut()
	}

	#[context("accessing mutable vector data (dropping blob)")]
	/// Mutably borrow the vector content.
	///
	/// Any existing blob is dropped because it would be stale after mutation.
	pub fn as_vector_mut(&mut self) -> Result<&mut VectorTile> {
		self.as_content_mut()?.as_vector_mut()
	}

	/// Return the current tile **format** (e.g., `PNG`, `MVT`).
	pub fn format(&self) -> TileFormat {
		self.format
	}
	/// Return the current transport **compression** (e.g., `Uncompressed`, `Gzip`).
	pub fn compression(&self) -> TileCompression {
		self.compression
	}

	#[context("changing format: {:?} -> {:?} (q={:?}, s={:?})", self.format, format, quality, speed)]
	/// Change the tile's **format** (e.g., `PNG` → `WEBP`) while preserving the content type.
	///
	/// The tile's content is materialized; the existing blob is dropped; and optional
	/// `quality`/`speed` hints are updated if provided (passed as `Some`).
	/// Passing `None` keeps the previous hint value.
	///
	/// The `format` must have the same type (raster vs. vector) as the current format.
	pub fn change_format(&mut self, format: TileFormat, quality: Option<u8>, speed: Option<u8>) -> Result<()> {
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

	#[context("changing compression to {:?}", compression)]
	/// Update the outer **compression** flag and (if a blob is present) re-compress it.
	///
	/// When no blob is present, only the flag changes; compression is applied on the
	/// next materialization of the blob.
	pub fn change_compression(&mut self, compression: TileCompression) -> Result<()> {
		if self.blob.is_some() {
			self.recompress_blob(compression)?;
		} else {
			self.compression = compression;
		}
		Ok(())
	}

	/// Whether the tile currently holds an encoded blob.
	pub fn has_blob(&self) -> bool {
		self.blob.is_some()
	}

	/// Whether the tile currently holds decoded content.
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

#[cfg(test)]
mod tests {
	use super::*;
	use TileCompression::*;
	use TileFormat::*;
	use std::io::Cursor;
	use versatiles_image::{GenericImage, GenericImageView, ImageBuffer};

	fn tiny_rgb_image() -> DynamicImage {
		let data = vec![
			255, 0, 0, // red
			0, 255, 0, // green
			0, 0, 255, // blue
			10, 20, 30, // misc
		];
		DynamicImage::ImageRgb8(ImageBuffer::from_vec(2, 2, data).expect("vec->img"))
	}

	use rstest::rstest;

	#[rstest]
	#[case(Uncompressed, Uncompressed)]
	fn recompress_blob_noop_when_same(#[case] start: TileCompression, #[case] target: TileCompression) -> Result<()> {
		let mut tile = Tile::from_image(tiny_rgb_image(), PNG)?;
		// materialize and set to the desired starting compression
		let before = tile.as_blob(start)?.clone();
		assert!(tile.has_blob());
		assert_eq!(tile.compression(), start);

		// call the private method via test wrapper
		tile.__recompress_blob_for_test(target);
		assert!(tile.has_blob());
		assert_eq!(tile.compression(), target);

		// since start == target, bytes must be identical
		let after = tile.as_blob(Uncompressed)?.clone();
		assert_eq!(before, after);
		Ok(())
	}

	#[test]
	fn decompress_blob_noop_when_uncompressed() -> Result<()> {
		let mut tile = Tile::from_image(tiny_rgb_image(), PNG)?;
		let before = tile.as_blob(Uncompressed)?.clone();
		assert_eq!(tile.compression(), Uncompressed);

		// call the private method via test wrapper; nothing should change
		tile.__decompress_blob_for_test();
		assert_eq!(tile.compression(), Uncompressed);
		let after = tile.as_blob(Uncompressed)?.clone();
		assert_eq!(before, after);
		Ok(())
	}

	#[test]
	fn from_image_then_materialize_blob() -> Result<()> {
		let img = tiny_rgb_image();
		let mut tile = Tile::from_image(img, PNG)?;

		assert!(!tile.has_blob());
		assert!(tile.has_content());
		assert_eq!(tile.compression(), Uncompressed);

		// Force blob creation
		let blob = tile.as_blob(Uncompressed)?;
		assert!(!blob.is_empty());
		assert!(tile.has_blob());
		assert!(tile.has_content());
		assert_eq!(tile.compression(), Uncompressed);
		Ok(())
	}

	#[test]
	fn as_content_mut_deletes_blob() -> Result<()> {
		let mut tile = Tile::from_image(tiny_rgb_image(), PNG)?;
		assert!(!tile.has_blob());
		assert!(tile.has_content());

		// create blob first
		let _ = tile.as_blob(Uncompressed)?;
		assert!(tile.has_blob());
		assert!(tile.has_content());

		// now mutate content => blob must be dropped
		match tile.as_content_mut()? {
			TileContent::Raster(image) => image.put_pixel(0, 0, [1, 2, 3, 4].into()),
			_ => panic!("expected raster image"),
		}
		assert!(!tile.has_blob());
		assert!(tile.has_content());
		Ok(())
	}

	#[test]
	fn change_format_sets_flags() -> Result<()> {
		let mut tile = Tile::from_image(tiny_rgb_image(), PNG)?;
		tile.change_format(PNG, Some(77), None)?;
		assert_eq!(tile.format(), PNG);
		let _ = tile.as_blob(Uncompressed)?;

		// change only speed afterwards
		tile.change_format(PNG, None, Some(5))?;
		let _ = tile.as_blob(Uncompressed)?;
		Ok(())
	}

	#[test]
	fn cachevalue_roundtrip_preserves_fields() -> Result<()> {
		let img = tiny_rgb_image();
		let mut original = Tile::from_image(img, PNG)?;
		// produce a blob so both blob+content exist
		let _ = original.as_blob(Uncompressed)?;

		let mut buf = Vec::new();
		original.write_to_cache(&mut buf).expect("serialize");

		let mut cur = Cursor::new(buf.as_slice());
		let decoded = Tile::read_from_cache(&mut cur).expect("deserialize");

		// Both implement PartialEq
		assert_eq!(decoded, original);
		// Cursor must be fully consumed
		assert_eq!(cur.position(), buf.len() as u64);
		Ok(())
	}

	#[test]
	fn into_image_returns_raster() -> Result<()> {
		let img = tiny_rgb_image();
		let tile = Tile::from_image(img.clone(), PNG)?;
		assert_eq!(tile.into_image()?, img);
		Ok(())
	}

	#[test]
	fn from_blob_then_materialize_content_roundtrip() -> Result<()> {
		// Start with a raster tile and encode to an uncompressed blob
		let img = tiny_rgb_image();
		let blob = Tile::from_image(img.clone(), PNG)?.into_blob(Uncompressed)?;

		// Build a new tile from that blob; it should start without content
		let mut tile2 = Tile::from_blob(blob, Uncompressed, PNG);
		assert!(tile2.has_blob());
		assert!(!tile2.has_content());

		// Accessing image should materialize the content without dropping the blob
		let out = tile2.as_image()?.clone();
		assert_eq!(out.dimensions(), (2, 2));
		assert!(tile2.has_blob());
		assert!(tile2.has_content());
		assert_eq!(tile2.compression(), Uncompressed);
		Ok(())
	}

	#[test]
	fn as_blob_is_deterministic_when_unchanged() -> Result<()> {
		let mut tile = Tile::from_image(tiny_rgb_image(), PNG)?;
		let b1 = tile.as_blob(Uncompressed)?.clone();
		let b2 = tile.as_blob(Uncompressed)?.clone();
		assert_eq!(b1, b2);
		Ok(())
	}

	#[test]
	fn cache_roundtrip_with_blob_only() -> Result<()> {
		// Create a blob-only tile using from_blob
		let img = tiny_rgb_image();
		let blob = Tile::from_image(img, PNG)?.into_blob(Uncompressed)?;
		let original = Tile::from_blob(blob, Uncompressed, PNG);
		assert!(original.has_blob());
		assert!(!original.has_content());

		let mut buf = Vec::new();
		original.write_to_cache(&mut buf).expect("serialize");
		let mut cur = Cursor::new(buf.as_slice());
		let decoded = Tile::read_from_cache(&mut cur).expect("deserialize");
		assert_eq!(decoded, original);
		assert_eq!(cur.position(), buf.len() as u64);
		Ok(())
	}

	#[test]
	fn cache_roundtrip_with_content_only() -> Result<()> {
		let original = Tile::from_image(tiny_rgb_image(), PNG)?;
		assert!(!original.has_blob());
		assert!(original.has_content());

		let mut buf = Vec::new();
		original.write_to_cache(&mut buf).expect("serialize");
		let mut cur = Cursor::new(buf.as_slice());
		let decoded = Tile::read_from_cache(&mut cur).expect("deserialize");
		assert_eq!(decoded, original);
		assert_eq!(cur.position(), buf.len() as u64);
		Ok(())
	}

	// Helper that peeks format/quality/speed written by CacheValue without rehydrating the Tile
	fn read_format_quality_speed(buf: &[u8]) -> (TileFormat, TileCompression, Option<u8>, Option<u8>) {
		let mut cur = Cursor::new(buf);
		// Skip blob and content
		let _ = Option::<Blob>::read_from_cache(&mut cur).unwrap();
		let _ = Option::<TileContent>::read_from_cache(&mut cur).unwrap();
		let fmt = TileFormat::read_from_cache(&mut cur).unwrap();
		let comp = TileCompression::read_from_cache(&mut cur).unwrap();
		let q = Option::<u8>::read_from_cache(&mut cur).unwrap();
		let s = Option::<u8>::read_from_cache(&mut cur).unwrap();
		(fmt, comp, q, s)
	}

	#[test]
	fn change_format_none_keeps_existing_quality_and_speed() -> Result<()> {
		let mut tile = Tile::from_image(tiny_rgb_image(), PNG)?;
		// Set initial flags
		tile.change_format(PNG, Some(50), Some(10))?;
		let mut buf = Vec::new();
		tile.write_to_cache(&mut buf).unwrap();
		let (fmt1, _c1, q1, s1) = read_format_quality_speed(&buf);
		assert_eq!(fmt1, PNG);
		assert_eq!(q1, Some(50));
		assert_eq!(s1, Some(10));

		// Now call with None/None so flags should remain
		tile.change_format(PNG, None, None)?;
		buf.clear();
		tile.write_to_cache(&mut buf).unwrap();
		let (fmt2, _c2, q2, s2) = read_format_quality_speed(&buf);
		assert_eq!(fmt2, PNG);
		assert_eq!(q2, Some(50));
		assert_eq!(s2, Some(10));
		Ok(())
	}

	#[test]
	fn as_vector_on_vector_content_returns_ref() -> Result<()> {
		let vt = VectorTile::default();
		let mut tile = Tile::from_vector(vt.clone(), MVT)?;
		assert!(!tile.has_blob());
		assert!(tile.has_content());
		let got = tile.as_vector()?;
		// Can't compare references directly; make sure we can read without panic and content stays
		let _ = got as *const _; // use it
		assert!(tile.has_content());
		Ok(())
	}

	#[test]
	fn into_vector_consumes_and_returns_owned() -> Result<()> {
		let vt = VectorTile::default();
		let tile = Tile::from_vector(vt.clone(), MVT)?;
		let out = tile.into_vector()?;
		assert_eq!(out, vt);
		Ok(())
	}

	#[test]
	fn as_image_mut_modifies_and_drops_blob() -> Result<()> {
		let mut tile = Tile::from_image(tiny_rgb_image(), PNG)?;
		// ensure we have a blob to be dropped after mutation
		let _ = tile.as_blob(Uncompressed)?;
		assert!(tile.has_blob());

		assert_eq!(tile.as_image()?.dimensions(), (2, 2));

		tile.as_image_mut()?.put_pixel(0, 1, [42, 24, 12, 255].into());
		assert!(tile.has_content());
		assert!(!tile.has_blob());
		let p = tile.as_image()?.get_pixel(0, 1);
		assert_eq!(p.0[0..3], [42, 24, 12]);
		Ok(())
	}

	#[test]
	fn as_vector_mut_allows_mutation_and_keeps_content() -> Result<()> {
		let vt = VectorTile::default();
		let mut tile = Tile::from_vector(vt, MVT)?;
		assert!(!tile.has_blob());
		// We don't know vector internals here; taking &mut should materialize content and not panic
		let _vref: &mut VectorTile = tile.as_vector_mut()?;
		assert!(tile.has_content());
		assert!(!tile.has_blob());
		Ok(())
	}

	#[test]
	fn change_compression_on_existing_blob_noop_when_same() -> Result<()> {
		let mut tile = Tile::from_image(tiny_rgb_image(), PNG)?;
		// materialize an uncompressed blob
		let before = tile.as_blob(Uncompressed)?.clone();
		assert!(tile.has_blob());
		assert_eq!(tile.compression(), Uncompressed);

		// change to the same compression => should be a no-op
		tile.change_compression(Uncompressed)?;
		assert!(tile.has_blob());
		assert_eq!(tile.compression(), Uncompressed);
		let after = tile.as_blob(Uncompressed)?.clone();
		assert_eq!(before, after);

		// still decodable and content intact
		let img = tile.as_image()?;
		assert_eq!(img.dimensions(), (2, 2));
		Ok(())
	}

	#[test]
	fn change_compression_flag_persists_through_cache_without_blob() -> Result<()> {
		// No blob present; changing compression only flips the flag
		let mut tile = Tile::from_image(tiny_rgb_image(), PNG)?;
		assert!(!tile.has_blob());
		tile.change_compression(Uncompressed)?;
		assert_eq!(tile.compression(), Uncompressed);

		let mut buf = Vec::new();
		tile.write_to_cache(&mut buf).unwrap();
		let mut cur = Cursor::new(buf.as_slice());
		let decoded = Tile::read_from_cache(&mut cur).unwrap();
		assert_eq!(decoded.compression(), Uncompressed);
		Ok(())
	}

	#[test]
	fn debug_shows_core_fields_for_raster_content_only() -> Result<()> {
		let tile = Tile::from_image(tiny_rgb_image(), PNG)?;
		assert_eq!(
			format!("{tile:?}"),
			"Tile { has_blob: false, has_content: true, format: PNG, compression: Uncompressed }"
		);
		Ok(())
	}

	#[test]
	fn debug_shows_core_fields_for_blob_only() -> Result<()> {
		let blob = Tile::from_image(tiny_rgb_image(), PNG)?.into_blob(Uncompressed)?;
		let tile = Tile::from_blob(blob, Uncompressed, PNG);
		assert_eq!(
			format!("{tile:?}"),
			"Tile { has_blob: true, has_content: false, format: PNG, compression: Uncompressed }"
		);
		Ok(())
	}

	#[test]
	fn debug_shows_core_fields_for_vector_content_only() -> Result<()> {
		let vt = VectorTile::default();
		let mut tile = Tile::from_vector(vt, MVT)?;
		tile.change_compression(Gzip)?;
		assert_eq!(
			format!("{tile:?}"),
			"Tile { has_blob: false, has_content: true, format: MVT, compression: Gzip }"
		);
		Ok(())
	}
}
