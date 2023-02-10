use crate::helper::Blob;

use super::ByteRange;
use std::{
	fs::File,
	io::{BufWriter, Seek, SeekFrom, Write},
	path::Path,
};

trait VersaTilesDstTrait: Write + Seek + Send {}
impl VersaTilesDstTrait for BufWriter<File> {}

pub struct VersaTilesDst {
	writer: Box<dyn VersaTilesDstTrait>,
}
impl VersaTilesDst {
	pub fn new_file(filename: &Path) -> VersaTilesDst {
		VersaTilesDst {
			writer: Box::new(BufWriter::new(File::create(filename).unwrap())),
		}
	}
	pub fn append(&mut self, blob: &Blob) -> ByteRange {
		let pos = self.writer.stream_position().unwrap();
		let len = self.writer.write(blob.as_slice()).unwrap();

		ByteRange::new(pos, len as u64)
	}
	pub fn write_start(&mut self, blob: &Blob) {
		let pos = self.writer.stream_position().unwrap();
		self.writer.rewind().unwrap();
		self.writer.write_all(blob.as_slice()).unwrap();
		self.writer.seek(SeekFrom::Start(pos)).unwrap();
	}
	pub fn get_position(&mut self) -> u64 {
		self.writer.stream_position().unwrap()
	}
}
