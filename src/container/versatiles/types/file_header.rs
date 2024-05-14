#![allow(dead_code)]

use crate::types::{Blob, ByteRange, DataReader, TileCompression, TileFormat};
use anyhow::{bail, ensure, Result};
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};

const HEADER_LENGTH: usize = 66;
const BBOX_SCALE: i32 = 10000000;

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
	pub fn new(
		tile_format: &TileFormat, compression: &TileCompression, zoom_range: [u8; 2], bbox: &[f64; 4],
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
		ensure!(bbox[0] <= bbox[2], "bbox[0] ({}) <= bbox[2] ({})", bbox[0], bbox[2]);
		ensure!(bbox[1] <= bbox[3], "bbox[1] ({}) <= bbox[3] ({})", bbox[1], bbox[3]);

		Ok(FileHeader {
			zoom_range,
			bbox: bbox.map(|v| (v * BBOX_SCALE as f64) as i32),
			tile_format: *tile_format,
			compression: *compression,
			meta_range: ByteRange::empty(),
			blocks_range: ByteRange::empty(),
		})
	}

	pub async fn from_reader(reader: &mut DataReader) -> Result<FileHeader> {
		let range = ByteRange::new(0, HEADER_LENGTH as u64);
		let blob = reader.read_range(&range).await?;
		FileHeader::from_blob(blob)
	}

	pub fn to_blob(&self) -> Result<Blob> {
		let mut header: Vec<u8> = Vec::new();
		header.write_all(b"versatiles_v02")?;

		// tile type
		header.write_u8(match self.tile_format {
			TileFormat::BIN => 0x00,

			TileFormat::PNG => 0x10,
			TileFormat::JPG => 0x11,
			TileFormat::WEBP => 0x12,
			TileFormat::AVIF => 0x13,
			TileFormat::SVG => 0x14,

			TileFormat::PBF => 0x20,
			TileFormat::GEOJSON => 0x21,
			TileFormat::TOPOJSON => 0x22,
			TileFormat::JSON => 0x23,
		})?;

		// compression
		header.write_u8(match self.compression {
			TileCompression::None => 0,
			TileCompression::Gzip => 1,
			TileCompression::Brotli => 2,
		})?;

		header.write_u8(self.zoom_range[0])?;
		header.write_u8(self.zoom_range[1])?;

		header.write_i32::<BE>(self.bbox[0])?;
		header.write_i32::<BE>(self.bbox[1])?;
		header.write_i32::<BE>(self.bbox[2])?;
		header.write_i32::<BE>(self.bbox[3])?;

		self.meta_range.write_to_buf(&mut header)?;
		self.blocks_range.write_to_buf(&mut header)?;

		if header.len() != HEADER_LENGTH {
			bail!(
				"header should be {HEADER_LENGTH} bytes long, but is {} bytes long",
				header.len()
			);
		}

		Ok(Blob::from(header))
	}

	fn from_blob(blob: Blob) -> Result<FileHeader> {
		if blob.len() != HEADER_LENGTH {
			bail!("'{blob:?}' is not a valid versatiles header. A header should be {HEADER_LENGTH} bytes long.");
		}

		let mut header = Cursor::new(blob.as_slice());
		let mut magic_word = [0u8; 14];
		header.read_exact(&mut magic_word)?;
		if &magic_word != b"versatiles_v02" {
			bail!("'{blob:?}' is not a valid versatiles header. A header should start with 'versatiles_v02'");
		};

		let tile_format = match header.read_u8()? {
			0x00 => TileFormat::BIN,

			0x10 => TileFormat::PNG,
			0x11 => TileFormat::JPG,
			0x12 => TileFormat::WEBP,
			0x13 => TileFormat::AVIF,
			0x14 => TileFormat::SVG,

			0x20 => TileFormat::PBF,
			0x21 => TileFormat::GEOJSON,
			0x22 => TileFormat::TOPOJSON,
			0x23 => TileFormat::JSON,
			value => bail!("unknown tile_type value: {value}"),
		};

		let compression = match header.read_u8()? {
			0 => TileCompression::None,
			1 => TileCompression::Gzip,
			2 => TileCompression::Brotli,
			value => bail!("unknown compression value: {value}"),
		};

		let zoom_range: [u8; 2] = [header.read_u8()?, header.read_u8()?];

		let bbox: [i32; 4] = [
			header.read_i32::<BE>()?,
			header.read_i32::<BE>()?,
			header.read_i32::<BE>()?,
			header.read_i32::<BE>()?,
		];

		let meta_range = ByteRange::from_reader(&mut header)?;
		let blocks_range = ByteRange::from_reader(&mut header)?;

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
	use byteorder::ByteOrder;
	use std::panic::catch_unwind;

	#[test]
	#[allow(clippy::zero_prefixed_literal)]
	fn conversion() {
		let test = |tile_format: &TileFormat, compression: &TileCompression, a: u64, b: u64, c: u64, d: u64| {
			let mut header1 = FileHeader::new(tile_format, compression, [0, 0], &[0.0, 0.0, 0.0, 0.0]).unwrap();
			header1.meta_range = ByteRange::new(a, b);
			header1.blocks_range = ByteRange::new(c, d);

			let header2 = FileHeader::from_blob(header1.to_blob().unwrap()).unwrap();
			assert_eq!(header1, header2);
			assert_eq!(&header2.tile_format, tile_format);
			assert_eq!(&header2.compression, compression);
			assert_eq!(header2.meta_range, ByteRange::new(a, b));
			assert_eq!(header2.blocks_range, ByteRange::new(c, d));
		};
		test(
			&TileFormat::JPG,
			&TileCompression::None,
			314159265358979323,
			846264338327950288,
			419716939937510582,
			097494459230781640,
		);

		test(&TileFormat::PBF, &TileCompression::Brotli, 29, 97, 92, 458);
	}

	#[test]
	fn new_file_header() {
		let tf = TileFormat::PNG;
		let comp = TileCompression::Gzip;
		let zoom = [10, 14];
		let bbox = [-180.0, -85.0511, 180.0, 85.0511];
		let header = FileHeader::new(&tf, &comp, zoom, &bbox).unwrap();

		assert_eq!(header.zoom_range, zoom);
		assert_eq!(header.bbox, [-1800000000, -850511000, 1800000000, 850511000]);
		assert_eq!(header.tile_format, tf);
		assert_eq!(header.compression, comp);
		assert_eq!(header.meta_range, ByteRange::empty());
		assert_eq!(header.blocks_range, ByteRange::empty());
	}

	#[test]
	fn to_blob() {
		let header = FileHeader::new(
			&TileFormat::PBF,
			&TileCompression::Gzip,
			[3, 8],
			&[-180.0, -85.051_13, 180.0, 85.051_13],
		)
		.unwrap();

		let blob = header.to_blob().unwrap();

		assert_eq!(blob.len(), HEADER_LENGTH);
		assert_eq!(&blob.as_slice()[0..14], b"versatiles_v02");
		assert_eq!(blob.as_slice()[14], 0x20);
		assert_eq!(blob.as_slice()[15], 1);
		assert_eq!(blob.as_slice()[16], 3);
		assert_eq!(blob.as_slice()[17], 8);
		assert_eq!(BE::read_i32(&blob.as_slice()[18..22]), -1800000000);
		assert_eq!(BE::read_i32(&blob.as_slice()[22..26]), -850511300);
		assert_eq!(BE::read_i32(&blob.as_slice()[26..30]), 1800000000);
		assert_eq!(BE::read_i32(&blob.as_slice()[30..34]), 850511300);
		assert_eq!(
			ByteRange::from_buf(&blob.as_slice()[34..50]).unwrap(),
			ByteRange::empty()
		);
		assert_eq!(
			ByteRange::from_buf(&blob.as_slice()[50..66]).unwrap(),
			ByteRange::empty()
		);

		let header2 = FileHeader::from_blob(blob).unwrap();

		assert_eq!(header2.zoom_range, [3, 8]);
		assert_eq!(header2.bbox, [-1800000000, -850511300, 1800000000, 850511300]);
		assert_eq!(header2.tile_format, TileFormat::PBF);
		assert_eq!(header2.compression, TileCompression::Gzip);
		assert_eq!(header2.meta_range, ByteRange::empty());
		assert_eq!(header2.blocks_range, ByteRange::empty());
	}

	#[test]
	fn new_file_header_with_invalid_params() {
		let tf = TileFormat::PNG;
		let comp = TileCompression::Gzip;

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
		let compression = TileCompression::Gzip;
		let zoom_range = [0, 0];
		let bbox = [0.0, 0.0, 0.0, 0.0];

		let tile_formats = vec![
			TileFormat::BIN,
			TileFormat::PNG,
			TileFormat::JPG,
			TileFormat::WEBP,
			TileFormat::AVIF,
			TileFormat::SVG,
			TileFormat::PBF,
			TileFormat::GEOJSON,
			TileFormat::TOPOJSON,
			TileFormat::JSON,
		];

		for tile_format in tile_formats {
			let header = FileHeader::new(&tile_format, &compression, zoom_range, &bbox).unwrap();
			let blob = header.to_blob().unwrap();
			let header2 = FileHeader::from_blob(blob).unwrap();

			assert_eq!(&header2.tile_format, &tile_format);
			assert_eq!(&header2.compression, &compression);
		}
	}

	#[test]
	fn all_compressions() {
		let tile_format = TileFormat::PNG;
		let zoom_range = [0, 0];
		let bbox = [0.0, 0.0, 0.0, 0.0];

		let compressions = vec![TileCompression::None, TileCompression::Gzip, TileCompression::Brotli];

		for compression in compressions {
			let header = FileHeader::new(&tile_format, &compression, zoom_range, &bbox).unwrap();
			let blob = header.to_blob().unwrap();
			let header2 = FileHeader::from_blob(blob).unwrap();

			assert_eq!(&header2.tile_format, &tile_format);
			assert_eq!(&header2.compression, &compression);
		}
	}

	#[test]
	fn invalid_header_length() {
		let invalid_blob = Blob::from(vec![0; HEADER_LENGTH - 1]);
		let result = catch_unwind(|| {
			FileHeader::from_blob(invalid_blob).unwrap();
		});

		assert!(result.is_err());
	}

	#[test]
	fn invalid_magic_word() {
		let mut invalid_blob = Blob::from(vec![0; HEADER_LENGTH]);
		invalid_blob.as_mut_slice()[0..14].copy_from_slice(b"invalid_header");
		let result = catch_unwind(|| {
			FileHeader::from_blob(invalid_blob).unwrap();
		});

		assert!(result.is_err());
	}

	#[test]
	fn unknown_tile_format() {
		let mut invalid_blob = FileHeader::new(&TileFormat::PNG, &TileCompression::Gzip, [0, 0], &[0.0, 0.0, 0.0, 0.0])
			.unwrap()
			.to_blob()
			.unwrap();
		invalid_blob.as_mut_slice()[14] = 0xFF; // Set an unknown tile format value

		let result = catch_unwind(|| {
			FileHeader::from_blob(invalid_blob).unwrap();
		});

		assert!(result.is_err());
	}

	#[test]
	fn unknown_compression() {
		let mut invalid_blob = FileHeader::new(&TileFormat::PNG, &TileCompression::Gzip, [0, 0], &[0.0, 0.0, 0.0, 0.0])
			.unwrap()
			.to_blob()
			.unwrap();
		invalid_blob.as_mut_slice()[15] = 0xFF; // Set an unknown compression value

		let result = catch_unwind(|| {
			FileHeader::from_blob(invalid_blob).unwrap();
		});

		assert!(result.is_err());
	}
}
