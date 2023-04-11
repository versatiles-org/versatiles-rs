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

pub struct DataReaderFile {
	name: String,
	reader: BufReader<File>,
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
		let file = File::open(filename)?;

		Ok(Box::new(Self {
			name: source.to_string(),
			reader: BufReader::new(file),
		}))
	}
	async fn read_range(&mut self, range: &ByteRange) -> Result<Blob> {
		let mut buffer = vec![0; range.length as usize];

		self.reader.seek(SeekFrom::Start(range.offset))?;
		self.reader.read_exact(&mut buffer)?;

		return Ok(Blob::from(buffer));
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}
