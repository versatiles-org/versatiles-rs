use crate::opencloudtiles::lib::Blob;

use super::ByteRange;
use std::{
	fs::File,
	io::{BufWriter, Seek, SeekFrom, Write},
	path::Path,
};

trait CloudTilesDstTrait: Write + Seek + Send {}
impl CloudTilesDstTrait for BufWriter<File> {}

pub struct CloudTilesDst {
	writer: Box<dyn CloudTilesDstTrait>,
}
impl CloudTilesDst {
	pub fn new_file(filename: &Path) -> CloudTilesDst {
		CloudTilesDst {
			writer: Box::new(BufWriter::new(File::create(filename).unwrap())),
		}
	}
	pub fn append(&mut self, blob: Blob) -> ByteRange {
		let pos = self.writer.stream_position().unwrap();
		let len = self.writer.write(blob.as_slice()).unwrap();

		ByteRange::new(pos, len as u64)
	}
	pub fn write_start(&mut self, blob: Blob) {
		let pos = self.writer.stream_position().unwrap();
		self.writer.seek(SeekFrom::Start(0)).unwrap();

		self.writer.write_all(blob.as_slice()).unwrap();

		self.writer.seek(SeekFrom::Start(pos)).unwrap();
	}
}
