//! This module defines the `FileHeader` struct, which represents the header of a versatiles file.
//!
//! The `FileHeader` struct contains metadata about the file, including its tile format, compression, zoom range, bounding box, and byte ranges for metadata and blocks.

use anyhow::{Result, bail, ensure};
use versatiles_core::{
	Blob, ByteRange, GeoBBox, MAX_ZOOM_LEVEL, TileCompression, TileFormat,
	io::{DataReader, ValueReader, ValueReaderSlice, ValueWriter, ValueWriterBlob},
	utils::float_to_int,
};
use versatiles_derive::context;

const HEADER_LENGTH: u64 = 66;
const BBOX_SCALE: f64 = 10_000_000.0;

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
	#[context("Failed to create FileHeader")]
	pub fn new(
		tile_format: TileFormat,
		compression: TileCompression,
		zoom_range: [u8; 2],
		bbox: &GeoBBox,
	) -> Result<FileHeader> {
		check_zoom_range(zoom_range)?;

		let arr = bbox.as_array();
		Ok(FileHeader {
			zoom_range,
			bbox: [
				float_to_int(arr[0] * BBOX_SCALE)?,
				float_to_int(arr[1] * BBOX_SCALE)?,
				float_to_int(arr[2] * BBOX_SCALE)?,
				float_to_int(arr[3] * BBOX_SCALE)?,
			],
			tile_format,
			compression,
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
	#[context("Failed to read FileHeader from reader")]
	pub async fn from_reader(reader: &mut DataReader) -> Result<FileHeader> {
		let range = ByteRange::new(0, HEADER_LENGTH);
		let blob = reader.read_range(&range).await?;
		FileHeader::from_blob(&blob)
	}

	/// Converts the `FileHeader` to a binary blob.
	///
	/// # Errors
	/// Returns an error if the conversion fails.
	#[context("Failed to create FileHeader from blob")]
	pub fn to_blob(&self) -> Result<Blob> {
		use TileCompression::{Brotli, Gzip, Uncompressed, Zstd};
		use TileFormat::{AVIF, BIN, GEOJSON, JPG, JSON, MLT, MVT, PNG, SVG, TOPOJSON, WEBP};

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

			MVT => 0x20,
			GEOJSON => 0x21,
			TOPOJSON => 0x22,
			JSON => 0x23,

			MLT => 0x24,
		})?;

		// compression
		writer.write_u8(match self.compression {
			Uncompressed => 0,
			Gzip => 1,
			Brotli => 2,
			Zstd => 3,
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
	#[context("Failed to create FileHeader from blob")]
	fn from_blob(blob: &Blob) -> Result<FileHeader> {
		use TileCompression::{Brotli, Gzip, Uncompressed, Zstd};
		use TileFormat::{AVIF, BIN, GEOJSON, JPG, JSON, MLT, MVT, PNG, SVG, TOPOJSON, WEBP};

		if blob.len() != HEADER_LENGTH {
			bail!("'{blob:?}' is not a valid versatiles header. A header should be {HEADER_LENGTH} bytes long.");
		}

		let mut reader = ValueReaderSlice::new_be(blob.as_slice());
		let magic_word = reader.read_string(14)?;
		if &magic_word != "versatiles_v02" {
			bail!("'{blob:?}' is not a valid versatiles header. A header should start with 'versatiles_v02'");
		}

		let tile_format = match reader.read_u8()? {
			0x00 => BIN,

			0x10 => PNG,
			0x11 => JPG,
			0x12 => WEBP,
			0x13 => AVIF,
			0x14 => SVG,

			0x20 => MVT,
			0x21 => GEOJSON,
			0x22 => TOPOJSON,
			0x23 => JSON,
			0x24 => MLT,
			value => bail!("unknown tile_type value: {value}"),
		};

		let compression = match reader.read_u8()? {
			0 => Uncompressed,
			1 => Gzip,
			2 => Brotli,
			3 => Zstd,
			value => bail!("unknown compression value: {value}"),
		};

		let zoom_range: [u8; 2] = [reader.read_u8()?, reader.read_u8()?];
		check_zoom_range(zoom_range)?;

		let bbox: [i32; 4] = [
			reader.read_i32()?,
			reader.read_i32()?,
			reader.read_i32()?,
			reader.read_i32()?,
		];
		// Within -180..180 and -90..90, minimum at most maximum: a bbox that
		// crosses the antimeridian is not one the spec allows.
		let [x_min, y_min, x_max, y_max] = bbox.map(|value| f64::from(value) / BBOX_SCALE);
		GeoBBox::new(x_min, y_min, x_max, y_max).with_context(|| format!("the header's bbox {bbox:?} is invalid"))?;

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

/// Zoom levels run from 0 to 30, and the minimum is at most the maximum.
fn check_zoom_range([min, max]: [u8; 2]) -> Result<()> {
	ensure!(
		min <= max,
		"the minimum zoom level ({min}) must be <= the maximum ({max})"
	);
	ensure!(
		max <= MAX_ZOOM_LEVEL,
		"the maximum zoom level ({max}) must be <= {MAX_ZOOM_LEVEL}"
	);
	Ok(())
}

#[cfg(test)]
#[expect(
	clippy::cast_possible_truncation,
	reason = "test data is built from literal byte ranges"
)]
mod tests {
	use std::panic::catch_unwind;

	use TileCompression::*;

	use super::*;

	#[test]
	#[expect(
		clippy::zero_prefixed_literal,
		reason = "the four values are consecutive 18-digit runs of pi; the last is zero-padded to match"
	)]
	fn conversion() {
		let test = |tile_format: TileFormat, compression: TileCompression, a: u64, b: u64, c: u64, d: u64| {
			let mut header1 = FileHeader::new(
				tile_format,
				compression,
				[0, 0],
				&GeoBBox::new(0.0, 0.0, 0.0, 0.0).unwrap(),
			)
			.unwrap();
			header1.meta_range = ByteRange::new(a, b);
			header1.blocks_range = ByteRange::new(c, d);

			let header2 = FileHeader::from_blob(&header1.to_blob().unwrap()).unwrap();
			assert_eq!(header1, header2);
			assert_eq!(header2.tile_format, tile_format);
			assert_eq!(header2.compression, compression);
			assert_eq!(header2.meta_range, ByteRange::new(a, b));
			assert_eq!(header2.blocks_range, ByteRange::new(c, d));
		};
		test(
			TileFormat::JPG,
			Uncompressed,
			314159265358979323,
			846264338327950288,
			419716939937510582,
			097494459230781640,
		);

		test(TileFormat::MVT, Brotli, 29, 97, 92, 458);
	}

	#[test]
	fn new_file_header() {
		let tf = TileFormat::PNG;
		let comp = Gzip;
		let zoom = [10, 14];
		let bbox = GeoBBox::new(-180.0, -85.0511, 180.0, 85.0511).unwrap();
		let header = FileHeader::new(tf, comp, zoom, &bbox).unwrap();

		assert_eq!(header.zoom_range, zoom);
		assert_eq!(header.bbox, [-1800000000, -850511000, 1800000000, 850511000]);
		assert_eq!(header.tile_format, tf);
		assert_eq!(header.compression, comp);
		assert_eq!(header.meta_range, ByteRange::empty());
		assert_eq!(header.blocks_range, ByteRange::empty());
	}

	#[test]
	fn to_blob() -> Result<()> {
		let header = FileHeader::new(
			TileFormat::MVT,
			Gzip,
			[3, 8],
			&GeoBBox::new(-180.0, -85.051_13, 180.0, 85.051_13)?,
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
		assert_eq!(header2.bbox, [-1800000000, -850511300, 1800000000, 850511300]);
		assert_eq!(header2.tile_format, TileFormat::MVT);
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
			assert!(
				catch_unwind(|| {
					FileHeader::new(tf, comp, zoom, &GeoBBox::try_from(&bbox).unwrap()).unwrap();
				})
				.is_err()
			);
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
		let bbox = GeoBBox::new(0.0, 0.0, 0.0, 0.0).unwrap();

		let tile_formats = vec![BIN, PNG, JPG, WEBP, AVIF, SVG, MVT, GEOJSON, TOPOJSON, JSON, MLT];

		for tile_format in tile_formats {
			let header = FileHeader::new(tile_format, compression, zoom_range, &bbox).unwrap();
			let blob = header.to_blob().unwrap();
			let header2 = FileHeader::from_blob(&blob).unwrap();

			assert_eq!(header2.tile_format, tile_format);
			assert_eq!(header2.compression, compression);
		}
	}

	/// The code the spec gives MLT, in both directions.
	#[test]
	fn mlt_is_0x24() -> Result<()> {
		let header = FileHeader::new(
			TileFormat::MLT,
			Uncompressed,
			[0, 14],
			&GeoBBox::new(0.0, 0.0, 0.0, 0.0)?,
		)?;
		let blob = header.to_blob()?;
		assert_eq!(blob.as_slice()[14], 0x24);
		assert_eq!(FileHeader::from_blob(&blob)?.tile_format, TileFormat::MLT);
		Ok(())
	}

	/// A header written with `fields` changed at their offsets, as a file with
	/// a header no writer of ours produces would have it.
	fn patched(fields: &[(usize, &[u8])]) -> Blob {
		let mut blob = FileHeader::new(
			TileFormat::PNG,
			Gzip,
			[0, 14],
			&GeoBBox::new(-10.0, -10.0, 10.0, 10.0).unwrap(),
		)
		.unwrap()
		.to_blob()
		.unwrap();
		for (offset, bytes) in fields {
			blob.as_mut_slice()[*offset..*offset + bytes.len()].copy_from_slice(bytes);
		}
		blob
	}

	fn i32_bytes(degrees: f64) -> [u8; 4] {
		float_to_int::<f64, i32>(degrees * BBOX_SCALE).unwrap().to_be_bytes()
	}

	/// The spec's ranges: longitude -180..180, latitude -90..90, minimum at
	/// most maximum on both axes, so no bbox crossing the antimeridian.
	#[rstest::rstest]
	#[case::longitude_below(18, -180.5)]
	#[case::latitude_below(22, -90.5)]
	#[case::longitude_above(26, 180.5)]
	#[case::latitude_above(30, 90.5)]
	#[case::crossing_the_antimeridian(18, 170.0)]
	#[case::min_latitude_above_max(22, 20.0)]
	fn an_invalid_bbox_is_rejected(#[case] offset: usize, #[case] degrees: f64) {
		let error = FileHeader::from_blob(&patched(&[(offset, &i32_bytes(degrees))])).unwrap_err();
		assert!(format!("{error:#}").contains("bbox"), "{error:#}");
	}

	#[test]
	fn the_full_extent_is_a_valid_bbox() -> Result<()> {
		let edges: [&[u8]; 4] = [
			&i32_bytes(-180.0),
			&i32_bytes(-90.0),
			&i32_bytes(180.0),
			&i32_bytes(90.0),
		];
		let blob = patched(&[(18, edges[0]), (22, edges[1]), (26, edges[2]), (30, edges[3])]);
		assert_eq!(
			FileHeader::from_blob(&blob)?.bbox,
			[-1800000000, -900000000, 1800000000, 900000000]
		);
		Ok(())
	}

	#[rstest::rstest]
	#[case::min_above_max([14, 10], "must be <= the maximum")]
	#[case::beyond_30([0, 31], "must be <= 30")]
	fn an_invalid_zoom_range_is_rejected(#[case] zoom: [u8; 2], #[case] reason: &str) {
		let error = FileHeader::from_blob(&patched(&[(16, &zoom)])).unwrap_err();
		assert!(format!("{error:#}").contains(reason), "{error:#}");

		let error = FileHeader::new(TileFormat::PNG, Gzip, zoom, &GeoBBox::new(0.0, 0.0, 0.0, 0.0).unwrap()).unwrap_err();
		assert!(format!("{error:#}").contains(reason), "{error:#}");
	}

	#[test]
	fn all_compressions() {
		let tile_format = TileFormat::PNG;
		let zoom_range = [0, 0];
		let bbox = GeoBBox::new(0.0, 0.0, 0.0, 0.0).unwrap();

		let compressions = vec![Uncompressed, Gzip, Brotli, Zstd];

		for compression in compressions {
			let header = FileHeader::new(tile_format, compression, zoom_range, &bbox).unwrap();
			let blob = header.to_blob().unwrap();
			let header2 = FileHeader::from_blob(&blob).unwrap();

			assert_eq!(header2.tile_format, tile_format);
			assert_eq!(header2.compression, compression);
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
		let mut invalid_blob = FileHeader::new(
			TileFormat::PNG,
			Gzip,
			[0, 0],
			&GeoBBox::new(0.0, 0.0, 0.0, 0.0).unwrap(),
		)
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
		let mut invalid_blob = FileHeader::new(
			TileFormat::PNG,
			Gzip,
			[0, 0],
			&GeoBBox::new(0.0, 0.0, 0.0, 0.0).unwrap(),
		)
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
