use super::*;
use crate::helper::*;
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};

#[derive(Debug, PartialEq)]
pub struct FileHeader {
	pub zoom_range: [u8; 2],
	pub bbox: [f32; 4],

	pub tile_format: TileFormat,
	pub precompression: Precompression,

	pub meta_range: ByteRange,
	pub blocks_range: ByteRange,
}
impl FileHeader {
	pub fn new(tile_format: &TileFormat, precompression: &Precompression) -> FileHeader {
		FileHeader {
			zoom_range: [0, 0],
			bbox: [0.0, 0.0, 0.0, 0.0],
			tile_format: tile_format.clone(),
			precompression: precompression.to_owned(),
			meta_range: ByteRange::empty(),
			blocks_range: ByteRange::empty(),
		}
	}
	pub fn from_reader(reader: &mut Box<dyn VersaTilesSrcTrait>) -> FileHeader {
		FileHeader::from_blob(reader.read_range(&ByteRange::new(0, 66)))
	}
	pub fn to_blob(&self) -> Blob {
		let mut header: Vec<u8> = Vec::new();
		header.write_all(b"versatiles-Container-v1:").unwrap();

		// tile type
		header
			.write_u8(match self.tile_format {
				TileFormat::PNG => 0,
				TileFormat::JPG => 1,
				TileFormat::WEBP => 2,
				TileFormat::PBF => 16,
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

		header.write_f32::<BE>(self.bbox[0]).unwrap();
		header.write_f32::<BE>(self.bbox[1]).unwrap();
		header.write_f32::<BE>(self.bbox[2]).unwrap();
		header.write_f32::<BE>(self.bbox[3]).unwrap();

		self.meta_range.write_to_buf(&mut header);
		self.blocks_range.write_to_buf(&mut header);

		if header.len() != 66 {
			panic!()
		}

		Blob::from_vec(header)
	}
	fn from_blob(blob: Blob) -> FileHeader {
		if blob.len() != 66 {
			panic!();
		}

		let mut header = Cursor::new(blob.as_slice());
		let mut magic_word = [0u8; 14];
		header.read_exact(&mut magic_word).unwrap();
		if &magic_word != b"versatiles_v01" {
			panic!()
		};

		let tile_type = header.read_u8().unwrap();
		let compression = header.read_u8().unwrap();

		let tile_format = match tile_type {
			0 => TileFormat::PNG,
			1 => TileFormat::JPG,
			2 => TileFormat::WEBP,
			3 => TileFormat::PBF,
			_ => panic!(),
		};

		let precompression = match compression {
			0 => Precompression::Uncompressed,
			1 => Precompression::Gzip,
			2 => Precompression::Brotli,
			_ => panic!(),
		};

		let zoom_range: [u8; 2] = [header.read_u8().unwrap(), header.read_u8().unwrap()];

		let bbox: [f32; 4] = [
			header.read_f32::<BE>().unwrap(),
			header.read_f32::<BE>().unwrap(),
			header.read_f32::<BE>().unwrap(),
			header.read_f32::<BE>().unwrap(),
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
		let test = |tile_format: &TileFormat,
		            precompression: &Precompression,
		            a: u64,
		            b: u64,
		            c: u64,
		            d: u64| {
			let mut header1 = FileHeader::new(tile_format, precompression);
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
