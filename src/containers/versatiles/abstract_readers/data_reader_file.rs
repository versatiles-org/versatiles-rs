use super::super::types::ByteRange;
use super::DataReaderTrait;
use crate::shared::{Blob, Error, Result};
use async_trait::async_trait;
use std::{
	env::current_dir,
	fs::File,
	io::{BufReader, Read, Seek, SeekFrom},
	path::Path,
};
use tokio::sync::Mutex;

pub struct DataReaderFile {
	name: String,
	reader_mutex: Mutex<BufReader<File>>,
}

#[async_trait]
impl DataReaderTrait for DataReaderFile {
	async fn new(source: &str) -> Result<Box<Self>> {
		let mut filename = current_dir()?;
		filename.push(Path::new(source));

		if !filename.exists() {
			return Err(Error::new(&format!("file \"{filename:?}\" not found")));
		}

		if !filename.is_absolute() {
			return Err(Error::new(&format!("filename {filename:?} must be absolute")));
		}

		filename = filename.canonicalize()?;

		Ok(Box::new(Self {
			name: source.to_string(),
			reader_mutex: Mutex::new(BufReader::new(File::open(filename)?)),
		}))
	}
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		let mut buffer = vec![0; range.length as usize];
		let mut reader_safe = self.reader_mutex.lock().await;

		reader_safe.seek(SeekFrom::Start(range.offset))?;
		reader_safe.read_exact(&mut buffer)?;

		return Ok(Blob::from(buffer));
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}
