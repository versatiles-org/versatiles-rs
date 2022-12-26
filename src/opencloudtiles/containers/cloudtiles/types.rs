use crate::opencloudtiles::types::TileBBox;
use crate::types::TileFormat;
use byteorder::BigEndian as BE;
use byteorder::{ReadBytesExt, WriteBytesExt};
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
		ByteRange { offset: 0, length: 0 }
	}
	pub fn from_buf(reader: &mut impl Read) -> ByteRange {
		ByteRange::new(reader.read_u64::<BE>().unwrap(), reader.read_u64::<BE>().unwrap())
	}
	pub fn write_to_buf(&self, writer: &mut impl WriteBytesExt) {
		writer.write_u64::<BE>(self.offset).unwrap();
		writer.write_u64::<BE>(self.length).unwrap();
	}
}

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
		file.seek(std::io::SeekFrom::Start(0)).unwrap();
		file.write(&self.to_bytes()).unwrap();
		file.seek(std::io::SeekFrom::Start(current_pos)).unwrap();
	}
	pub fn read(file: &mut BufReader<File>) -> FileHeader {
		let current_pos = file.stream_position().unwrap();
		file.seek(std::io::SeekFrom::Start(0)).unwrap();

		let mut header = vec![0; 62];
		file.read_exact(&mut header).unwrap();
		file.seek(std::io::SeekFrom::Start(current_pos)).unwrap();

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

pub struct BlockDefinition {
	pub level: u64,
	pub block_x: u64,
	pub block_y: u64,
	pub bbox: TileBBox,
	pub count: u64,
}

pub struct BlockIndex {
	buffer: Vec<u8>,
	count: usize,
}

impl BlockIndex {
	pub fn new() -> BlockIndex {
		return BlockIndex {
			buffer: Vec::new(),
			count: 0,
		};
	}
	pub fn add(&mut self, level: u8, col: u32, row: u32, range: &ByteRange) {
		self.buffer.write_u8(level as u8).unwrap();
		self.buffer.write_u32::<BE>(col as u32).unwrap();
		self.buffer.write_u32::<BE>(row as u32).unwrap();
		self.buffer.write_u64::<BE>(range.offset).unwrap();
		self.buffer.write_u64::<BE>(range.length).unwrap();
		self.count += 1;
	}
	pub fn as_vec(&self) -> &Vec<u8> {
		if self.buffer.len() != self.count * 25 {
			panic!()
		}
		return &self.buffer;
	}
}

pub struct TileIndex {
	buffer: Vec<u8>,
	length: usize,
	count: usize,
}
unsafe impl Send for TileIndex {}

impl TileIndex {
	pub fn new(bbox: &TileBBox) -> TileIndex {
		let count = bbox.count_tiles() as usize;

		let length = count * 12 + 4;

		let mut buffer: Vec<u8> = Vec::with_capacity(length);
		buffer.resize(length, 0);

		let mut cursor = Cursor::new(&mut buffer);
		cursor.write_u8(bbox.x_min as u8).unwrap();
		cursor.write_u8(bbox.y_min as u8).unwrap();
		cursor.write_u8(bbox.x_max as u8).unwrap();
		cursor.write_u8(bbox.y_max as u8).unwrap();

		return TileIndex { buffer, length, count };
	}
	pub fn set(&mut self, index: usize, tile_byte_range: &ByteRange) {
		assert!(index < self.count, "index {} is to big for count {}", index, self.count);

		let pos = 4 + 12 * index;
		let slice_range = std::ops::Range {
			start: pos,
			end: pos + 12,
		};
		// println!("index {} pos {} slice_range {:?}", index, pos, slice_range);
		let mut slice = &mut self.buffer.as_mut_slice()[slice_range];
		slice.write_u64::<BE>(tile_byte_range.offset).unwrap();
		slice.write_u32::<BE>(tile_byte_range.length as u32).unwrap();
	}
	pub fn as_vec(&self) -> &Vec<u8> {
		if self.buffer.len() != self.length {
			panic!("{} != {}", self.buffer.len(), self.length);
		}
		return &self.buffer;
	}
}
