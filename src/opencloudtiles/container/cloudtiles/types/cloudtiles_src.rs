use crate::opencloudtiles::lib::Blob;

use super::ByteRange;
use std::{
	env::current_dir,
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
		CloudTilesSrc(Box::new(CloudTilesSrcFile::new(source)))
	}
	fn read_range(&self, range: &ByteRange) -> Blob {
		self.0.read_range(range)
	}
	fn get_name(&self) -> &str {
		self.0.get_name()
	}
}

struct CloudTilesSrcFile {
	name: String,
	reader_mutex: Mutex<BufReader<File>>,
}
impl CloudTilesSrcTrait for CloudTilesSrcFile {
	fn new(source: &str) -> Self {
		let mut filename = current_dir().unwrap();
		filename.push(Path::new(source));

		assert!(filename.exists(), "file {:?} does not exist", filename);
		assert!(
			filename.is_absolute(),
			"filename {:?} must be absolute",
			filename
		);

		filename = filename.canonicalize().unwrap();

		CloudTilesSrcFile {
			name: source.to_string(),
			reader_mutex: Mutex::new(BufReader::new(File::open(filename).unwrap())),
		}
	}
	fn read_range(&self, range: &ByteRange) -> Blob {
		let mut buffer = vec![0; range.length as usize];
		let mut reader_safe = self.reader_mutex.lock().unwrap();

		reader_safe.seek(SeekFrom::Start(range.offset)).unwrap();
		reader_safe.read_exact(&mut buffer).unwrap();

		Blob::from_vec(buffer)
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}
