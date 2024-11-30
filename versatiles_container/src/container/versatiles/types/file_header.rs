#![allow(dead_code)]

//! This module defines the `FileHeader` struct, which represents the header of a versatiles file.
//!
//! The `FileHeader` struct contains metadata about the file, including its tile format, compression, zoom range, bounding box, and byte ranges for metadata and blocks.

use anyhow::{bail, ensure, Result};
use versatiles_core::{types::*, utils::io::*};

const HEADER_LENGTH: u64 = 66;
const BBOX_SCALE: i32 = 10000000;

/// A struct representing the header of a versatiles file.
#[derive(Debug, PartialEq)]
pub struct FileHeader {
	pub zoom_range: [u8; 2],
	pub bbox: [i32; 4],
	pub tile_format: TileFormat,
	pub compression: TileCompression,
	pub meta_range: ByteRange,
	pub blocks_range: ByteRange,
}

impl FileHeader {
	/// Creates a new `FileHeader`.
	///
	/// # Arguments
	/// * `tile_format` - The format of the tiles in the file.
	/// * `compression` - The compression method used for the tiles.
	/// * `zoom_range` - The range of zoom levels in the file.
	/// * `bbox` - The bounding box of the tiles in the file.
	///
	/// # Errors
	/// Returns an error if the zoom range or bounding box is invalid.
	pub fn new(
		tile_format: &TileFormat,
		compression: &TileCompression,
		zoom_range: [u8; 2],
		bbox: &[f64; 4],
	) -> Result<FileHeader> {
		ensure!(
			zoom_range[0] <= zoom_range[1],
			"zoom_range[0] ({}) must be <= zoom_range[1] ({})",
			zoom_range[0],
			zoom_range[1]
		);
		ensure!(bbox[0] >= -180.0, "bbox[0] ({}) >= -180", bbox[0]);
		ensure!(bbox[1] >= -90.0, "bbox[1] ({}) >= -90", bbox[1]);
		ensure!(bbox[2] <= 180.0, "bbox[2] ({}) <= 180", bbox[2]);
		ensure!(bbox[3] <= 90.0, "bbox[3] ({}) <= 90", bbox[3]);
		ensure!(
			bbox[0] <= bbox[2],
			"bbox[0] ({}) <= bbox[2] ({})",
			bbox[0],
			bbox[2]
		);
		ensure!(
			bbox[1] <= bbox[3],
			"bbox[1] ({}) <= bbox[3] ({})",
			bbox[1],
			bbox[3]
		);

		Ok(FileHeader {
			zoom_range,
			bbox: bbox.map(|v| (v * BBOX_SCALE as f64) as i32),
			tile_format: *tile_format,
			compression: *compression,
			meta_range: ByteRange::empty(),
			blocks_range: ByteRange::empty(),
		})
	}

	/// Reads a `FileHeader` from a `DataReader`.
	///
	/// # Arguments
	/// * `reader` - The data reader to read from.
	///
	/// # Errors
	/// Returns an error if the header cannot be read or parsed correctly.
	pub async fn from_reader(reader: &mut DataReader) -> Result<FileHeader> {
		let range = ByteRange::new(0, HEADER_LENGTH);
		let blob = reader.read_range(&range).await?;
		FileHeader::from_blob(&blob)
	}

	/// Converts the `FileHeader` to a binary blob.
	///
	/// # Errors
	/// Returns an error if the conversion fails.
	pub fn to_blob(&self) -> Result<Blob> {
		use TileCompression::*;
		use TileFormat::*;

		let mut writer = ValueWriterBlob::new_be();
		writer.write_slice(b"versatiles_v02")?;

		// tile type
		writer.write_u8(match self.tile_format {
			BIN => 0x00,

			PNG => 0x10,
			JPG => 0x11,
			WEBP => 0x12,
			AVIF => 0x13,
			SVG => 0x14,

			PBF => 0x20,
			GEOJSON => 0x21,
			TOPOJSON => 0x22,
			JSON => 0x23,
		})?;

		// compression
		writer.write_u8(match self.compression {
			Uncompressed => 0,
			Gzip => 1,
			Brotli => 2,
		})?;

		writer.write_u8(self.zoom_range[0])?;
		writer.write_u8(self.zoom_range[1])?;

		writer.write_i32(self.bbox[0])?;
		writer.write_i32(self.bbox[1])?;
		writer.write_i32(self.bbox[2])?;
		writer.write_i32(self.bbox[3])?;

		writer.write_range(&self.meta_range)?;
		writer.write_range(&self.blocks_range)?;

		if writer.position()? != HEADER_LENGTH {
			bail!(
				"header should be {HEADER_LENGTH} bytes long, but is {} bytes long",
				writer.position()?
			);
		}

		Ok(writer.into_blob())
	}

	/// Creates a `FileHeader` from a binary blob.
	///
	/// # Arguments
	/// * `blob` - The binary data representing the file header.
	///
	/// # Errors
	/// Returns an error if the binary data cannot be parsed correctly.
	fn from_blob(blob: &Blob) -> Result<FileHeader> {
		use TileCompression::*;
		use TileFormat::*;

		if blob.len() != HEADER_LENGTH {
			bail!("'{blob:?}' is not a valid versatiles header. A header should be {HEADER_LENGTH} bytes long.");
		}

		let mut reader = ValueReaderSlice::new_be(blob.as_slice());
		let magic_word = reader.read_string(14)?;
		if &magic_word != "versatiles_v02" {
			bail!("'{blob:?}' is not a valid versatiles header. A header should start with 'versatiles_v02'");
		};

		let tile_format = match reader.read_u8()? {
			0x00 => BIN,

			0x10 => PNG,
			0x11 => JPG,
			0x12 => WEBP,
			0x13 => AVIF,
			0x14 => SVG,

			0x20 => PBF,
			0x21 => GEOJSON,
			0x22 => TOPOJSON,
			0x23 => JSON,
			value => bail!("unknown tile_type value: {value}"),
		};

		let compression = match reader.read_u8()? {
			0 => Uncompressed,
			1 => Gzip,
			2 => Brotli,
			value => bail!("unknown compression value: {value}"),
		};

		let zoom_range: [u8; 2] = [reader.read_u8()?, reader.read_u8()?];

		let bbox: [i32; 4] = [
			reader.read_i32()?,
			reader.read_i32()?,
			reader.read_i32()?,
			reader.read_i32()?,
		];

		let meta_range = reader.read_range()?;
		let blocks_range = reader.read_range()?;

		Ok(FileHeader {
			zoom_range,
			bbox,
			tile_format,
			compression,
			meta_range,
			blocks_range,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::panic::catch_unwind;
	use TileCompression::*;

	#[test]
	#[allow(clippy::zero_prefixed_literal)]
	fn conversion() {
		let test = |tile_format: &TileFormat,
		            compression: &TileCompression,
		            a: u64,
		            b: u64,
		            c: u64,
		            d: u64| {
			let mut header1 =
				FileHeader::new(tile_format, compression, [0, 0], &[0.0, 0.0, 0.0, 0.0]).unwrap();
			header1.meta_range = ByteRange::new(a, b);
			header1.blocks_range = ByteRange::new(c, d);

			let header2 = FileHeader::from_blob(&header1.to_blob().unwrap()).unwrap();
			assert_eq!(header1, header2);
			assert_eq!(&header2.tile_format, tile_format);
			assert_eq!(&header2.compression, compression);
			assert_eq!(header2.meta_range, ByteRange::new(a, b));
			assert_eq!(header2.blocks_range, ByteRange::new(c, d));
		};
		test(
			&TileFormat::JPG,
			&Uncompressed,
			314159265358979323,
			846264338327950288,
			419716939937510582,
			097494459230781640,
		);

		test(&TileFormat::PBF, &Brotli, 29, 97, 92, 458);
	}

	#[test]
	fn new_file_header() {
		let tf = TileFormat::PNG;
		let comp = Gzip;
		let zoom = [10, 14];
		let bbox = [-180.0, -85.0511, 180.0, 85.0511];
		let header = FileHeader::new(&tf, &comp, zoom, &bbox).unwrap();

		assert_eq!(header.zoom_range, zoom);
		assert_eq!(
			header.bbox,
			[-1800000000, -850511000, 1800000000, 850511000]
		);
		assert_eq!(header.tile_format, tf);
		assert_eq!(header.compression, comp);
		assert_eq!(header.meta_range, ByteRange::empty());
		assert_eq!(header.blocks_range, ByteRange::empty());
	}

	#[test]
	fn to_blob() -> Result<()> {
		let header = FileHeader::new(
			&TileFormat::PBF,
			&Gzip,
			[3, 8],
			&[-180.0, -85.051_13, 180.0, 85.051_13],
		)?;

		let blob = header.to_blob()?;
		let mut reader = ValueReaderSlice::new_be(blob.as_slice());

		assert_eq!(blob.len(), HEADER_LENGTH);
		assert_eq!(reader.read_string(14)?, "versatiles_v02");
		assert_eq!(reader.read_u8()?, 0x20);
		assert_eq!(reader.read_u8()?, 1);
		assert_eq!(reader.read_u8()?, 3);
		assert_eq!(reader.read_u8()?, 8);
		assert_eq!(reader.read_i32()?, -1800000000);
		assert_eq!(reader.read_i32()?, -850511300);
		assert_eq!(reader.read_i32()?, 1800000000);
		assert_eq!(reader.read_i32()?, 850511300);
		assert_eq!(reader.read_range()?, ByteRange::empty());
		assert_eq!(reader.read_range()?, ByteRange::empty());

		let header2 = FileHeader::from_blob(&blob)?;

		assert_eq!(header2.zoom_range, [3, 8]);
		assert_eq!(
			header2.bbox,
			[-1800000000, -850511300, 1800000000, 850511300]
		);
		assert_eq!(header2.tile_format, TileFormat::PBF);
		assert_eq!(header2.compression, Gzip);
		assert_eq!(header2.meta_range, ByteRange::empty());
		assert_eq!(header2.blocks_range, ByteRange::empty());

		Ok(())
	}

	#[test]
	fn new_file_header_with_invalid_params() {
		let tf = TileFormat::PNG;
		let comp = Gzip;

		let should_panic = |zoom: [u8; 2], bbox: [f64; 4]| {
			assert!(catch_unwind(|| {
				FileHeader::new(&tf, &comp, zoom, &bbox).unwrap();
			})
			.is_err())
		};

		should_panic([14, 10], [0.0, 0.0, 0.0, 0.0]);
		should_panic([0, 0], [-190.0, -85.0511, 180.0, 85.0511]);
		should_panic([0, 0], [-180.0, -95.0511, 180.0, 85.0511]);
		should_panic([0, 0], [-180.0, -85.0511, 190.0, 85.0511]);
		should_panic([0, 0], [-180.0, -85.0511, 180.0, 95.0511]);
		should_panic([0, 0], [-180.0, 85.0511, 180.0, -85.0511]);
		should_panic([0, 0], [180.0, -85.0511, -180.0, 85.0511]);
	}

	#[test]
	fn all_tile_formats() {
		use TileFormat::*;

		let compression = Gzip;
		let zoom_range = [0, 0];
		let bbox = [0.0, 0.0, 0.0, 0.0];

		let tile_formats = vec![BIN, PNG, JPG, WEBP, AVIF, SVG, PBF, GEOJSON, TOPOJSON, JSON];

		for tile_format in tile_formats {
			let header = FileHeader::new(&tile_format, &compression, zoom_range, &bbox).unwrap();
			let blob = header.to_blob().unwrap();
			let header2 = FileHeader::from_blob(&blob).unwrap();

			assert_eq!(&header2.tile_format, &tile_format);
			assert_eq!(&header2.compression, &compression);
		}
	}

	#[test]
	fn all_compressions() {
		let tile_format = TileFormat::PNG;
		let zoom_range = [0, 0];
		let bbox = [0.0, 0.0, 0.0, 0.0];

		let compressions = vec![Uncompressed, Gzip, Brotli];

		for compression in compressions {
			let header = FileHeader::new(&tile_format, &compression, zoom_range, &bbox).unwrap();
			let blob = header.to_blob().unwrap();
			let header2 = FileHeader::from_blob(&blob).unwrap();

			assert_eq!(&header2.tile_format, &tile_format);
			assert_eq!(&header2.compression, &compression);
		}
	}

	#[test]
	fn invalid_header_length() {
		let invalid_blob = Blob::from(vec![0; HEADER_LENGTH as usize - 1]);
		assert!(FileHeader::from_blob(&invalid_blob).is_err());
	}

	#[test]
	fn invalid_magic_word() {
		let mut invalid_blob = Blob::from(vec![0; HEADER_LENGTH as usize]);
		invalid_blob.as_mut_slice()[0..14].copy_from_slice(b"invalid_header");
		assert!(FileHeader::from_blob(&invalid_blob).is_err());
	}

	#[test]
	fn unknown_tile_format() {
		let mut invalid_blob =
			FileHeader::new(&TileFormat::PNG, &Gzip, [0, 0], &[0.0, 0.0, 0.0, 0.0])
				.unwrap()
				.to_blob()
				.unwrap();
		invalid_blob.as_mut_slice()[14] = 0xFF; // Set an unknown tile format value

		let result = catch_unwind(|| {
			FileHeader::from_blob(&invalid_blob).unwrap();
		});

		assert!(result.is_err());
	}

	#[test]
	fn unknown_compression() {
		let mut invalid_blob =
			FileHeader::new(&TileFormat::PNG, &Gzip, [0, 0], &[0.0, 0.0, 0.0, 0.0])
				.unwrap()
				.to_blob()
				.unwrap();
		invalid_blob.as_mut_slice()[15] = 0xFF; // Set an unknown compression value

		let result = catch_unwind(|| {
			FileHeader::from_blob(&invalid_blob).unwrap();
		});

		assert!(result.is_err());
	}
}
