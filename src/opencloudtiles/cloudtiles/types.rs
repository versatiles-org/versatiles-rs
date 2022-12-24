use bytes::{Buf, BufMut};

use crate::types::TileFormat;
use std::{
	fs::File,
	io::{BufReader, BufWriter, Cursor, Read, Seek, Write},
};

#[derive(Clone)]
pub struct ByteRange {
	pub offset: u64,
	pub length: u64,
}
impl ByteRange {
	pub fn new(offset: u64, length: u64) -> ByteRange {
		ByteRange { offset, length }
	}
	pub fn empty() -> ByteRange {
		ByteRange {
			offset: 0,
			length: 0,
		}
	}
	pub fn from_buf(reader: &mut impl Buf) -> ByteRange {
		ByteRange::new(reader.get_u64(), reader.get_u64())
	}
	pub fn write_to_buf(&self, writer: &mut impl BufMut) {
		writer.put_u64(self.offset);
		writer.put_u64(self.length);
	}
}

pub struct FileHeaderV1 {
	pub tile_format: TileFormat,
	pub meta_range: ByteRange,
	pub blocks_range: ByteRange,
}
impl FileHeaderV1 {
	pub fn new(tile_format: &TileFormat) -> FileHeaderV1 {
		return FileHeaderV1 {
			tile_format: tile_format.clone(),
			meta_range: ByteRange::empty(),
			blocks_range: ByteRange::empty(),
		};
	}
	pub fn write(&self, file: &mut BufWriter<File>) {
		let current_pos = file.stream_position().unwrap();
		file.seek(std::io::SeekFrom::Start(0)).unwrap();
		file.write(&self.to_bytes()).unwrap();
		file.seek(std::io::SeekFrom::Start(current_pos)).unwrap();
	}
	pub fn read(file: &mut BufReader<File>) -> FileHeaderV1 {
		let current_pos = file.stream_position().unwrap();
		file.seek(std::io::SeekFrom::Start(0)).unwrap();

		let mut header = vec![0; 62];
		file.read_exact(&mut header).unwrap();
		file.seek(std::io::SeekFrom::Start(current_pos)).unwrap();

		return FileHeaderV1::from_buffer(header.as_mut_slice());
	}
	fn to_bytes(&self) -> Vec<u8> {
		let mut header: Vec<u8> = Vec::new();
		header.put(&b"OpenCloudTiles-Container-v1:"[..]);

		// tile type
		header.put_u8(match self.tile_format {
			TileFormat::PNG => 0,
			TileFormat::JPG => 1,
			TileFormat::WEBP => 2,
			TileFormat::PBF | TileFormat::PBFGzip | TileFormat::PBFBrotli => 16,
		});

		// precompression
		header.put_u8(match self.tile_format {
			TileFormat::PNG | TileFormat::JPG | TileFormat::WEBP | TileFormat::PBF => 0,
			TileFormat::PBFGzip => 1,
			TileFormat::PBFBrotli => 2,
		});

		self.meta_range.write_to_buf(&mut header);
		self.blocks_range.write_to_buf(&mut header);

		if header.len() != 62 {
			panic!()
		}

		return header;
	}
	fn from_buffer(buf: &mut [u8]) -> FileHeaderV1 {
		if buf.len() != 62 {
			panic!();
		}

		let mut header = Cursor::new(buf);
		if header.copy_to_bytes(28) != "OpenCloudTiles-Container-v1:" {
			panic!()
		};

		let tile_type = header.get_u8();
		let compression = header.get_u8();

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

		return FileHeaderV1 {
			tile_format,
			meta_range,
			blocks_range,
		};
	}
}

pub struct BlockDefinition {
	pub level: u64,
	pub block_row: u64,
	pub block_col: u64,
	pub row_min: u64,
	pub row_max: u64,
	pub col_min: u64,
	pub col_max: u64,
	pub count: u64,
}

pub struct BlockIndex {
	cursor: Cursor<Vec<u8>>,
}

impl BlockIndex {
	pub fn new() -> BlockIndex {
		let data = Vec::new();
		let cursor = Cursor::new(data);
		return BlockIndex { cursor };
	}
	pub fn add(&mut self, level: &u64, row: &u64, col: &u64, range: &ByteRange) {
		self.cursor.write(&level.to_le_bytes()).unwrap();
		self.cursor.write(&row.to_le_bytes()).unwrap();
		self.cursor.write(&col.to_le_bytes()).unwrap();
		self.cursor.write(&range.offset.to_le_bytes()).unwrap();
		self.cursor.write(&range.length.to_le_bytes()).unwrap();
	}
	pub fn as_vec(&self) -> &Vec<u8> {
		return self.cursor.get_ref();
	}
}

pub struct TileIndex {
	cursor: Cursor<Vec<u8>>,
}
unsafe impl Send for TileIndex {}

impl TileIndex {
	pub fn new(row_min: u64, row_max: u64, col_min: u64, col_max: u64) -> TileIndex {
		let data = Vec::new();
		let mut cursor = Cursor::new(data);
		cursor.write(&(row_min as u8).to_le_bytes()).unwrap();
		cursor.write(&(row_max as u8).to_le_bytes()).unwrap();
		cursor.write(&(col_min as u8).to_le_bytes()).unwrap();
		cursor.write(&(col_max as u8).to_le_bytes()).unwrap();
		return TileIndex { cursor };
	}
	pub fn set(&mut self, index: u64, range: &ByteRange) {
		let new_position = 12 * index + 4;
		//if newPosition != self.cursor.stream_position().unwrap() {
		//	panic!();
		//}
		self.cursor.set_position(new_position);
		self.cursor.write(&range.offset.to_le_bytes()).unwrap();
		self.cursor.write(&range.length.to_le_bytes()).unwrap();
	}
	pub fn as_vec(&self) -> &Vec<u8> {
		return self.cursor.get_ref();
	}
}
