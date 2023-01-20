use super::ByteRange;
use crate::opencloudtiles::lib::Blob;
use core::panic;
use futures::executor::block_on;
use log::error;
use object_store::ObjectStore;
use std::{
	env::current_dir,
	fs::File,
	io::{BufReader, Read, Seek, SeekFrom},
	path::Path,
	sync::{Arc, Mutex},
};

pub trait CloudTilesSrcTrait {
	fn new(source: &str) -> Option<Self>
	where
		Self: Sized;
	fn read_range(&self, range: &ByteRange) -> Blob;
	fn get_name(&self) -> &str;
}

pub fn new_cloud_tile_src(source: &str) -> Box<dyn CloudTilesSrcTrait> {
	if let Some(src) = CloudTilesSrcObjectStore::new(source) {
		return Box::new(src);
	} else if let Some(src) = CloudTilesSrcFile::new(source) {
		return Box::new(src);
	}
	panic!();
}

struct CloudTilesSrcFile {
	name: String,
	reader_mutex: Mutex<BufReader<File>>,
}
impl CloudTilesSrcTrait for CloudTilesSrcFile {
	fn new(source: &str) -> Option<Self> {
		let mut filename = current_dir().unwrap();
		filename.push(Path::new(source));

		if !filename.exists() {
			error!("file {:?} does not exist", filename);
			return None;
		}

		assert!(
			filename.is_absolute(),
			"filename {filename:?} must be absolute"
		);

		filename = filename.canonicalize().unwrap();

		Some(Self {
			name: source.to_string(),
			reader_mutex: Mutex::new(BufReader::new(File::open(filename).unwrap())),
		})
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

struct CloudTilesSrcObjectStore {
	name: String,
	url: object_store::path::Path,
	object_store: Arc<dyn ObjectStore>,
}
impl CloudTilesSrcTrait for CloudTilesSrcObjectStore {
	fn new(source: &str) -> Option<Self> {
		let object_store = if source.starts_with("gs://") {
			object_store::gcp::GoogleCloudStorageBuilder::new()
				.with_service_account_path("credentials.json")
				.with_url(source)
				.build()
				.unwrap()
		} else {
			return None;
		};

		Some(Self {
			name: source.to_string(),
			url: object_store::path::Path::from(source),
			object_store: Arc::new(object_store),
		})
	}
	fn read_range(&self, range: &ByteRange) -> Blob {
		Blob::from_bytes(
			block_on(
				self
					.object_store
					.get_range(&self.url, range.as_range_usize()),
			)
			.unwrap(),
		)
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}
