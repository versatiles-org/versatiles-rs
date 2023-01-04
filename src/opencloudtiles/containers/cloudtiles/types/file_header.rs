use super::{ByteRange, CloudTilesSrc};
use crate::opencloudtiles::types::{Compression, TileFormat};
use byteorder::{ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};

#[derive(Debug)]
pub struct FileHeader {
	pub tile_format: TileFormat,
	pub precompression: Compression,
	pub meta_range: ByteRange,
	pub blocks_range: ByteRange,
}
impl FileHeader {
	pub fn new(tile_format: &TileFormat, precompression: &Compression) -> FileHeader {
		return FileHeader {
			tile_format: tile_format.clone(),
			precompression: precompression.clone(),
			meta_range: ByteRange::empty(),
			blocks_range: ByteRange::empty(),
		};
	}
	pub fn from_reader(reader: &mut CloudTilesSrc) -> FileHeader {
		let mut header = reader.read_range(&ByteRange::new(0, 62));
		return FileHeader::from_buffer(header.as_mut_slice());
	}
	pub fn to_bytes(&self) -> Vec<u8> {
		let mut header: Vec<u8> = Vec::new();
		header.write(b"OpenCloudTiles-Container-v1:").unwrap();

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
				Compression::Uncompressed => 0,
				Compression::Gzip => 1,
				Compression::Brotli => 2,
			})
			.unwrap();

		self.meta_range.write_to_buf(&mut header);
		self.blocks_range.write_to_buf(&mut header);

		if header.len() != 62 {
			panic!()
		}

		return header;
	}
	fn from_buffer(buf: &mut [u8]) -> FileHeader {
		if buf.len() != 62 {
			panic!();
		}

		let mut header = Cursor::new(buf);
		let mut magic_word = [0u8; 28];
		header.read_exact(&mut magic_word).unwrap();
		if &magic_word != b"OpenCloudTiles-Container-v1:" {
			panic!()
		};

		let tile_type = header.read_u8().unwrap();
		let compression = header.read_u8().unwrap();

		let tile_format = match tile_type {
			0 => TileFormat::PNG,
			1 => TileFormat::JPG,
			2 => TileFormat::WEBP,
			16 => TileFormat::PBF,
			_ => panic!(),
		};

		let precompression = match compression {
			0 => Compression::Uncompressed,
			1 => Compression::Gzip,
			2 => Compression::Brotli,
			_ => panic!(),
		};

		let meta_range = ByteRange::from_buf(&mut header);
		let blocks_range = ByteRange::from_buf(&mut header);

		return FileHeader {
			tile_format,
			precompression,
			meta_range,
			blocks_range,
		};
	}
}
