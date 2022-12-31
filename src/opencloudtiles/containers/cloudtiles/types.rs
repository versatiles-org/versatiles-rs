use crate::opencloudtiles::compress::{compress_brotli, decompress_brotli};
use crate::opencloudtiles::types::{TileBBox, TileBBoxPyramide};
use crate::types::TileFormat;
use byteorder::BigEndian as BE;
use byteorder::{ReadBytesExt, WriteBytesExt};
use std::io::{BufReader, SeekFrom};
use std::ops::Div;
use std::path::PathBuf;
use std::{
	fs::File,
	io::{BufWriter, Cursor, Read, Seek, Write},
};

trait CloudTilesSrcTrait: Read + Seek {}
impl CloudTilesSrcTrait for BufReader<File> {}

pub struct CloudTilesSrc {
	reader: Box<dyn CloudTilesSrcTrait>,
}
impl CloudTilesSrc {
	pub fn from_file(filename: &PathBuf) -> CloudTilesSrc {
		return CloudTilesSrc {
			reader: Box::new(BufReader::new(File::open(filename).unwrap())),
		};
	}
	pub fn read_range(&mut self, range: &ByteRange) -> Vec<u8> {
		let mut buffer = vec![0; range.length as usize];
		self.reader.seek(SeekFrom::Start(range.offset)).unwrap();
		self.reader.read_exact(&mut buffer).unwrap();
		return buffer;
	}
}

#[derive(Clone, Debug)]
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

pub struct BlockDefinition {
	pub level: u64,
	pub x: u64,
	pub y: u64,
	pub bbox: TileBBox,
	pub tile_range: ByteRange,
}
impl BlockDefinition {
	fn from_vec(buf: &[u8]) -> BlockDefinition {
		let mut cursor = Cursor::new(buf);
		let level = cursor.read_u8().unwrap() as u64;
		let x = cursor.read_u32::<BE>().unwrap() as u64;
		let y = cursor.read_u32::<BE>().unwrap() as u64;
		let x_min = cursor.read_u8().unwrap() as u64;
		let y_min = cursor.read_u8().unwrap() as u64;
		let x_max = cursor.read_u8().unwrap() as u64;
		let y_max = cursor.read_u8().unwrap() as u64;
		let bbox = TileBBox::new(x_min, y_min, x_max, y_max);
		let offset = cursor.read_u64::<BE>().unwrap();
		let length = cursor.read_u64::<BE>().unwrap();
		let tile_range = ByteRange::new(offset, length);
		return BlockDefinition {
			level,
			x,
			y,
			bbox,
			tile_range,
		};
	}
	pub fn count_tiles(&self) -> u64 {
		return self.bbox.count_tiles();
	}

	pub fn as_vec(&self) -> Vec<u8> {
		let vec = Vec::new();
		let mut cursor = Cursor::new(vec);
		cursor.write_u8(self.level as u8).unwrap();
		cursor.write_u32::<BE>(self.x as u32).unwrap();
		cursor.write_u32::<BE>(self.y as u32).unwrap();
		cursor.write_u8(self.bbox.x_min as u8).unwrap();
		cursor.write_u8(self.bbox.y_min as u8).unwrap();
		cursor.write_u8(self.bbox.x_max as u8).unwrap();
		cursor.write_u8(self.bbox.y_max as u8).unwrap();
		cursor.write_u64::<BE>(self.tile_range.offset).unwrap();
		cursor.write_u64::<BE>(self.tile_range.length).unwrap();
		return cursor.into_inner();
	}
}

pub struct BlockIndex {
	blocks: Vec<BlockDefinition>,
}
impl BlockIndex {
	pub fn new_empty() -> BlockIndex {
		return BlockIndex { blocks: Vec::new() };
	}
	pub fn from_vec(buf: &Vec<u8>) -> BlockIndex {
		let count = buf.len().div(29);
		assert_eq!(
			count * 29,
			buf.len(),
			"block index is defect, cause buffer length is not a multiple of 29"
		);
		let mut blocks = Vec::new();
		for i in 0..count {
			let block = BlockDefinition::from_vec(&buf[i * 29..(i + 1) * 29]);
			blocks.push(block);
		}
		println!("{}", buf.len());
		return BlockIndex { blocks };
	}
	pub fn from_brotli_vec(buf: &Vec<u8>) -> BlockIndex {
		let temp = &decompress_brotli(buf);
		return BlockIndex::from_vec(temp);
	}
	pub fn get_bbox_pyramide(&self) -> TileBBoxPyramide {
		let mut pyramide = TileBBoxPyramide::new_empty();
		for block in self.blocks.iter() {
			pyramide.include_bbox(block.level, &block.bbox);
		}
		return pyramide;
	}
	pub fn add_block(&mut self, block: BlockDefinition) {
		self.blocks.push(block)
	}
	pub fn as_vec(&self) -> Vec<u8> {
		let vec = Vec::new();
		let mut cursor = Cursor::new(vec);
		for block in self.blocks.iter() {
			let vec = block.as_vec();
			let slice = vec.as_slice();
			//println!("{}", slice.len());
			cursor.write(slice).unwrap();
		}
		return cursor.into_inner();
	}
	pub fn as_brotli_vec(&self) -> Vec<u8> {
		return compress_brotli(&self.as_vec());
	}
}

pub struct TileIndex {
	buffer: Vec<u8>,
	length: usize,
	count: usize,
}
unsafe impl Send for TileIndex {}

impl TileIndex {
	pub fn create(count: usize) -> TileIndex {
		let length = count * 12 + 4;

		let mut buffer: Vec<u8> = Vec::with_capacity(length);
		buffer.resize(length, 0);

		return TileIndex {
			buffer,
			length,
			count,
		};
	}
	pub fn set(&mut self, index: usize, tile_byte_range: &ByteRange) {
		assert!(
			index < self.count,
			"index {} is to big for count {}",
			index,
			self.count
		);

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
	pub fn as_brotli_vec(&self) -> Vec<u8> {
		return compress_brotli(&self.as_vec());
	}
}
