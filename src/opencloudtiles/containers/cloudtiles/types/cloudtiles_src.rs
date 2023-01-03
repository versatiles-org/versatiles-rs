use super::ByteRange;
use std::{
	fs::File,
	io::{BufReader, Read, Seek, SeekFrom},
	path::PathBuf,
};

trait CloudTilesSrcTrait: Read + Seek {}
impl CloudTilesSrcTrait for BufReader<File> {}

pub struct CloudTilesSrc {
	name: String,
	reader: Box<dyn CloudTilesSrcTrait>,
}
impl CloudTilesSrc {
	pub fn from_file(filename: &PathBuf) -> CloudTilesSrc {
		return CloudTilesSrc {
			name: filename.to_string_lossy().to_string(),
			reader: Box::new(BufReader::new(File::open(filename).unwrap())),
		};
	}
	pub fn read_range(&mut self, range: &ByteRange) -> Vec<u8> {
		let mut buffer = vec![0; range.length as usize];
		self.reader.seek(SeekFrom::Start(range.offset)).unwrap();
		self.reader.read_exact(&mut buffer).unwrap();
		return buffer;
	}
	pub fn get_name(&self) -> &str {
		return &self.name;
	}
}
