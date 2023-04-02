use super::ByteRange;
use async_trait::async_trait;
use reqwest::{Client, Method, Request, Url};
use std::{
	env::current_dir,
	fs::File,
	io::{BufReader, Read, Seek, SeekFrom},
	path::Path,
};
use tokio::sync::Mutex;
use versatiles_shared::{Blob, Error};

#[async_trait]
pub trait VersaTilesSrcTrait: Send + Sync {
	fn new(source: &str) -> Result<Self, Error>
	where
		Self: Sized;
	async fn read_range(&self, range: &ByteRange) -> Result<Blob, Error>;
	fn get_name(&self) -> &str;
}

pub fn new_versatiles_src(source: &str) -> Result<Box<dyn VersaTilesSrcTrait>, Error> {
	let start = source.split_terminator(':').next();

	Ok(match start {
		//Some("gs") => Box::new(VersaTilesSrcObjectStore::new(source)?),
		Some("http" | "https") => Box::new(VersaTilesSrcHttp::new(source)?),
		_ => Box::new(VersaTilesSrcFile::new(source)?),
	})
}

struct VersaTilesSrcFile {
	name: String,
	reader_mutex: Mutex<BufReader<File>>,
}
#[async_trait]
impl VersaTilesSrcTrait for VersaTilesSrcFile {
	fn new(source: &str) -> Result<Self, Error> {
		let mut filename = current_dir()?;
		filename.push(Path::new(source));

		if !filename.exists() {
			return Err(Error::new(format!("file \"{filename:?}\" not found")));
		}

		if !filename.is_absolute() {
			return Err(Error::new(format!("filename {filename:?} must be absolute")));
		}

		filename = filename.canonicalize()?;

		Ok(Self {
			name: source.to_string(),
			reader_mutex: Mutex::new(BufReader::new(File::open(filename)?)),
		})
	}
	async fn read_range(&self, range: &ByteRange) -> Result<Blob, Error> {
		let mut buffer = vec![0; range.length as usize];
		let mut reader_safe = self.reader_mutex.lock().await;

		reader_safe.seek(SeekFrom::Start(range.offset))?;
		reader_safe.read_exact(&mut buffer)?;

		return Ok(Blob::from_vec(buffer));
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}

/*
struct VersaTilesSrcObjectStore {
	name: String,
	url: object_store::path::Path,
	object_store: Arc<dyn ObjectStore>,
}
#[async_trait]
impl VersaTilesSrcTrait for VersaTilesSrcObjectStore {
	fn new(source: &str) -> Result<Self, Error> {
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
	async fn read_range(&self, range: &ByteRange) -> Result<Blob, Error> {
		let bytes = block_on(self.object_store.get_range(&self.url, range.as_range_usize()))?;
		Ok(Blob::from_bytes(bytes))
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}
*/

struct VersaTilesSrcHttp {
	name: String,
	url: Url,
	client: Client,
}
#[async_trait]
impl VersaTilesSrcTrait for VersaTilesSrcHttp {
	fn new(source: &str) -> Result<Self, Error> {
		if source.starts_with("https://") || source.starts_with("http://") {
			Ok(Self {
				name: source.to_string(),
				url: Url::parse(source)?,
				client: Client::new(),
			})
		} else {
			Err(Error::new(format!(
				"source {} must start with http:// or https://",
				source
			)))
		}
	}
	async fn read_range(&self, range: &ByteRange) -> Result<Blob, Error> {
		let mut request = Request::new(Method::GET, self.url.clone());
		let request_range: String = format!("bytes={}-{}", range.offset, range.length + range.offset - 1);
		request.headers_mut().append("range", request_range.parse()?);
		//println!("### request {:?}", request);

		let result = Client::execute(&self.client, request).await?;
		//println!("### result {:?}", result);

		let bytes = result.bytes().await?;

		//let range = result.headers().get("content-range");
		//println!("range {:?}", range);

		Ok(Blob::from_bytes(bytes))
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}
