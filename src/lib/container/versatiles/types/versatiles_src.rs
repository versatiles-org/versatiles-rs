use super::ByteRange;
use crate::helper::Blob;
use futures::executor::block_on;
use log::error;
use object_store::ObjectStore;
use reqwest::{
	blocking::{Client, Request},
	Method, Url,
};
use std::{
	env::current_dir,
	fs::File,
	io::{BufReader, Read, Seek, SeekFrom},
	path::Path,
	sync::{Arc, Mutex},
};

pub trait VersaTilesSrcTrait {
	fn new(source: &str) -> Option<Self>
	where
		Self: Sized;
	fn read_range(&self, range: &ByteRange) -> Blob;
	fn get_name(&self) -> &str;
}

pub fn new_versatiles_src(source: &str) -> Box<dyn VersaTilesSrcTrait> {
	return match source.split_terminator(':').next() {
		Some("gs") => Box::new(VersaTilesSrcObjectStore::new(source).unwrap()),
		Some("http" | "https") => Box::new(VersaTilesSrcHttp::new(source).unwrap()),
		_ => Box::new(VersaTilesSrcFile::new(source).unwrap()),
	};
}

struct VersaTilesSrcFile {
	name: String,
	reader_mutex: Mutex<BufReader<File>>,
}
impl VersaTilesSrcTrait for VersaTilesSrcFile {
	fn new(source: &str) -> Option<Self> {
		let mut filename = current_dir().unwrap();
		filename.push(Path::new(source));

		if !filename.exists() {
			error!("file \"{:?}\" not found", filename);
			return None;
		}

		assert!(filename.is_absolute(), "filename {filename:?} must be absolute");

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

struct VersaTilesSrcObjectStore {
	name: String,
	url: object_store::path::Path,
	object_store: Arc<dyn ObjectStore>,
}
impl VersaTilesSrcTrait for VersaTilesSrcObjectStore {
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
		Blob::from_bytes(block_on(self.object_store.get_range(&self.url, range.as_range_usize())).unwrap())
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}

struct VersaTilesSrcHttp {
	name: String,
	url: Url,
	client: Client,
}
impl VersaTilesSrcTrait for VersaTilesSrcHttp {
	fn new(source: &str) -> Option<Self> {
		if source.starts_with("https://") || source.starts_with("http://") {
			Some(Self {
				name: source.to_string(),
				url: Url::parse(source).unwrap(),
				client: Client::new(),
			})
		} else {
			None
		}
	}
	fn read_range(&self, range: &ByteRange) -> Blob {
		let mut request = Request::new(Method::GET, self.url.clone());
		request.headers_mut().append(
			"range",
			format!("bytes={}-{}", range.offset, range.length + range.offset - 1)
				.parse()
				.unwrap(),
		);
		let response = Client::execute(&self.client, request).unwrap();
		Blob::from_bytes(response.bytes().unwrap())
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}
