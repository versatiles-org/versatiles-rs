use super::ByteRange;
use std::{
	fs::File,
	io::{BufWriter, SeekFrom},
	io::{Seek, Write},
	path::PathBuf,
};

trait CloudTilesDstTrait: Write + Seek + Send {}
impl CloudTilesDstTrait for BufWriter<File> {}

pub struct CloudTilesDst {
	writer: Box<dyn CloudTilesDstTrait>,
}
impl CloudTilesDst {
	pub fn new_file(filename: &PathBuf) -> CloudTilesDst {
		return CloudTilesDst {
			writer: Box::new(BufWriter::new(File::create(filename).unwrap())),
		};
	}
	pub fn append(&mut self, buf: &[u8]) -> ByteRange {
		let pos = self.writer.stream_position().unwrap();
		let len = self.writer.write(buf).unwrap();
		return ByteRange::new(pos, len as u64);
	}
	pub fn write_start(&mut self, buf: &[u8]) {
		let pos = self.writer.stream_position().unwrap();
		self.writer.seek(SeekFrom::Start(0)).unwrap();

		self.writer.write(buf).unwrap();

		self.writer.seek(SeekFrom::Start(pos)).unwrap();
	}
}
