use crate::opencloudtiles::types::Blob;

use super::ByteRange;
use std::{
	fs::File,
	io::{BufReader, Read, Seek, SeekFrom},
	path::Path,
	sync::Mutex,
};

pub trait CloudTilesSrcTrait {
	fn new(source: &str) -> Self
	where
		Self: Sized;
	fn read_range(&self, range: &ByteRange) -> Blob;
	fn get_name(&self) -> &str;
}

pub struct CloudTilesSrc(Box<dyn CloudTilesSrcTrait>);
impl CloudTilesSrcTrait for CloudTilesSrc {
	fn new(source: &str) -> Self
	where
		Self: Sized,
	{
		return CloudTilesSrc(Box::new(CloudTilesSrcFile::new(source)));
	}
	fn read_range(&self, range: &ByteRange) -> Blob {
		return self.0.read_range(range);
	}
	fn get_name(&self) -> &str {
		return self.0.get_name();
	}
}

struct CloudTilesSrcFile {
	name: String,
	reader_mutex: Mutex<BufReader<File>>,
}
impl CloudTilesSrcTrait for CloudTilesSrcFile {
	fn new(source: &str) -> Self {
		let path = Path::new(source);
		if !path.exists() {
			panic!("file {} does not exists", source)
		}

		return CloudTilesSrcFile {
			name: source.to_string(),
			reader_mutex: Mutex::new(BufReader::new(File::open(path).unwrap())),
		};
	}
	fn read_range(&self, range: &ByteRange) -> Blob {
		let mut buffer = vec![0; range.length as usize];
		let mut reader_safe = self.reader_mutex.lock().unwrap();

		reader_safe.seek(SeekFrom::Start(range.offset)).unwrap();
		reader_safe.read_exact(&mut buffer).unwrap();

		return Blob::from_vec(buffer);
	}
	fn get_name(&self) -> &str {
		return &self.name;
	}
}
