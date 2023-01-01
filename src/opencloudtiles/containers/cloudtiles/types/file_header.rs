use super::{ByteRange, CloudTilesSrc};
use crate::types::TileFormat;
use byteorder::{ReadBytesExt, WriteBytesExt};
use std::{
	fs::File,
	io::SeekFrom,
	io::{BufWriter, Cursor, Read, Seek, Write},
};

#[derive(Debug)]
pub struct FileHeader {
	pub tile_format: TileFormat,
	pub meta_range: ByteRange,
	pub blocks_range: ByteRange,
}
impl FileHeader {
	pub fn new(tile_format: &TileFormat) -> FileHeader {
		return FileHeader {
			tile_format: tile_format.clone(),
			meta_range: ByteRange::empty(),
			blocks_range: ByteRange::empty(),
		};
	}
	pub fn write(&self, file: &mut BufWriter<File>) {
		let current_pos = file.stream_position().unwrap();
		file.seek(SeekFrom::Start(0)).unwrap();
		file.write(&self.to_bytes()).unwrap();
		file.seek(SeekFrom::Start(current_pos)).unwrap();
	}
	pub fn read(reader: &mut CloudTilesSrc) -> FileHeader {
		let mut header = reader.read_range(&ByteRange::new(0, 62));
		return FileHeader::from_buffer(header.as_mut_slice());
	}
	fn to_bytes(&self) -> Vec<u8> {
		let mut header: Vec<u8> = Vec::new();
		header.write(b"OpenCloudTiles-Container-v1:").unwrap();

		// tile type
		header
			.write_u8(match self.tile_format {
				TileFormat::PNG => 0,
				TileFormat::JPG => 1,
				TileFormat::WEBP => 2,
				TileFormat::PBF | TileFormat::PBFGzip | TileFormat::PBFBrotli => 16,
			})
			.unwrap();

		// precompression
		header
			.write_u8(match self.tile_format {
				TileFormat::PNG | TileFormat::JPG | TileFormat::WEBP | TileFormat::PBF => 0,
				TileFormat::PBFGzip => 1,
				TileFormat::PBFBrotli => 2,
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

		let tile_format = match (tile_type, compression) {
			(0, 0) => TileFormat::PNG,
			(1, 0) => TileFormat::JPG,
			(2, 0) => TileFormat::WEBP,
			(16, 0) => TileFormat::PBF,
			(16, 1) => TileFormat::PBFGzip,
			(16, 2) => TileFormat::PBFBrotli,
			_ => panic!(),
		};

		let meta_range = ByteRange::from_buf(&mut header);
		let blocks_range = ByteRange::from_buf(&mut header);

		return FileHeader {
			tile_format,
			meta_range,
			blocks_range,
		};
	}
}
