use super::*;
use crate::helper::*;
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};

const HEADER_LENGTH: usize = 66;
const BBOX_SCALE: i32 = 10000000;

#[derive(Debug, PartialEq)]
pub struct FileHeader {
	pub zoom_range: [u8; 2],
	pub bbox: [i32; 4],

	pub tile_format: TileFormat,
	pub precompression: Precompression,

	pub meta_range: ByteRange,
	pub blocks_range: ByteRange,
}
impl FileHeader {
	pub fn new(
		tile_format: &TileFormat, precompression: &Precompression, zoom_range: [u8; 2], bbox: [f64; 4],
	) -> FileHeader {
		assert!(
			zoom_range[0] <= zoom_range[1],
			"zoom_range[0] ({}) must be <= zoom_range[1] ({})",
			zoom_range[0],
			zoom_range[1]
		);
		assert!(bbox[0] >= -180.0, "bbox[0] ({}) >= -180", bbox[0]);
		assert!(bbox[1] >= -90.0, "bbox[1] ({}) >= -90", bbox[1]);
		assert!(bbox[2] <= 180.0, "bbox[2] ({}) <= 180", bbox[2]);
		assert!(bbox[3] <= 90.0, "bbox[3] ({}) <= 90", bbox[3]);
		assert!(bbox[0] <= bbox[2], "bbox[0] ({}) <= bbox[2] ({})", bbox[0], bbox[2]);
		assert!(bbox[1] <= bbox[3], "bbox[1] ({}) <= bbox[3] ({})", bbox[1], bbox[3]);

		FileHeader {
			zoom_range,
			bbox: bbox.map(|v| (v * BBOX_SCALE as f64) as i32),
			tile_format: tile_format.clone(),
			precompression: precompression.to_owned(),
			meta_range: ByteRange::empty(),
			blocks_range: ByteRange::empty(),
		}
	}

	pub async fn from_reader(reader: &mut Box<dyn VersaTilesSrcTrait>) -> FileHeader {
		FileHeader::from_blob(
			reader
				.read_range(&ByteRange::new(0, HEADER_LENGTH as u64))
				.await
				.unwrap(),
		)
	}

	pub fn to_blob(&self) -> Blob {
		let mut header: Vec<u8> = Vec::new();
		header.write_all(b"versatiles_v02").unwrap();

		// tile type
		header
			.write_u8(match self.tile_format {
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
			})
			.unwrap();

		// precompression
		header
			.write_u8(match self.precompression {
				Precompression::Uncompressed => 0,
				Precompression::Gzip => 1,
				Precompression::Brotli => 2,
			})
			.unwrap();

		header.write_u8(self.zoom_range[0]).unwrap();
		header.write_u8(self.zoom_range[1]).unwrap();

		header.write_i32::<BE>(self.bbox[0]).unwrap();
		header.write_i32::<BE>(self.bbox[1]).unwrap();
		header.write_i32::<BE>(self.bbox[2]).unwrap();
		header.write_i32::<BE>(self.bbox[3]).unwrap();

		self.meta_range.write_to_buf(&mut header);
		self.blocks_range.write_to_buf(&mut header);

		if header.len() != HEADER_LENGTH {
			panic!(
				"header should be {} bytes long, but is {} bytes long",
				HEADER_LENGTH,
				header.len()
			)
		}

		Blob::from_vec(header)
	}

	fn from_blob(blob: Blob) -> FileHeader {
		if blob.len() != HEADER_LENGTH {
			panic!();
		}

		let mut header = Cursor::new(blob.as_slice());
		let mut magic_word = [0u8; 14];
		header.read_exact(&mut magic_word).unwrap();
		if &magic_word != b"versatiles_v02" {
			panic!()
		};

		let tile_type = header.read_u8().unwrap();
		let compression = header.read_u8().unwrap();

		let tile_format = match tile_type {
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
			_ => panic!(),
		};

		let precompression = match compression {
			0 => Precompression::Uncompressed,
			1 => Precompression::Gzip,
			2 => Precompression::Brotli,
			_ => panic!(),
		};

		let zoom_range: [u8; 2] = [header.read_u8().unwrap(), header.read_u8().unwrap()];

		let bbox: [i32; 4] = [
			header.read_i32::<BE>().unwrap(),
			header.read_i32::<BE>().unwrap(),
			header.read_i32::<BE>().unwrap(),
			header.read_i32::<BE>().unwrap(),
		];

		let meta_range = ByteRange::from_reader(&mut header);
		let blocks_range = ByteRange::from_reader(&mut header);

		FileHeader {
			zoom_range,
			bbox,
			tile_format,
			precompression,
			meta_range,
			blocks_range,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn conversion() {
		let test = |tile_format: &TileFormat, precompression: &Precompression, a: u64, b: u64, c: u64, d: u64| {
			let mut header1 = FileHeader::new(tile_format, precompression, [0, 0], [0.0, 0.0, 0.0, 0.0]);
			header1.meta_range = ByteRange::new(a, b);
			header1.blocks_range = ByteRange::new(c, d);

			let header2 = FileHeader::from_blob(header1.to_blob());
			assert_eq!(header1, header2);
			assert_eq!(&header2.tile_format, tile_format);
			assert_eq!(&header2.precompression, precompression);
			assert_eq!(header2.meta_range, ByteRange::new(a, b));
			assert_eq!(header2.blocks_range, ByteRange::new(c, d));
		};
		test(
			&TileFormat::JPG,
			&Precompression::Uncompressed,
			314159265358979323,
			846264338327950288,
			419716939937510582,
			097494459230781640,
		);

		test(&TileFormat::PBF, &Precompression::Brotli, 29, 97, 92, 458);
	}
}
