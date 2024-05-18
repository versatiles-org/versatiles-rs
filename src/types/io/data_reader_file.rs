use super::types::DataReaderTrait;
use crate::types::{Blob, ByteRange};
use anyhow::{ensure, Result};
use async_trait::async_trait;
use std::{
	fs::File,
	io::{BufReader, Read, Seek, SeekFrom},
	path::Path,
};

#[derive(Debug)]
pub struct DataReaderFile {
	name: String,
	reader: BufReader<File>,
}

impl DataReaderFile {
	pub fn open(path: &Path) -> Result<Box<DataReaderFile>> {
		ensure!(path.exists(), "file {path:?} does not exist");
		ensure!(path.is_absolute(), "path {path:?} must be absolute");
		ensure!(path.is_file(), "path {path:?} must be a file");

		let path = path.canonicalize()?;

		let file = File::open(&path)?;

		Ok(Box::new(DataReaderFile {
			name: path.to_str().unwrap().to_owned(),
			reader: BufReader::new(file),
		}))
	}
}

#[async_trait]
impl DataReaderTrait for DataReaderFile {
	async fn read_range(&mut self, range: &ByteRange) -> Result<Blob> {
		let mut buffer = vec![0; range.length as usize];

		self.reader.seek(SeekFrom::Start(range.offset))?;
		self.reader.read_exact(&mut buffer)?;

		Ok(Blob::from(buffer))
	}
	async fn read_all(&mut self) -> Result<Blob> {
		let mut buffer = vec![];
		self.reader.seek(SeekFrom::Start(0))?;
		self.reader.read_to_end(&mut buffer)?;
		Ok(Blob::from(buffer))
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}

impl Read for DataReaderFile {
	fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
		self.reader.read(buf)
	}
}

#[cfg(test)]
mod tests {
	use crate::assert_wildcard;

	use super::*;
	use anyhow::Result;
	use assert_fs::NamedTempFile;
	use std::{fs::File, io::Write};

	// Test the 'new' method for valid and invalid files
	#[tokio::test]
	async fn new() -> Result<()> {
		let temp_file_path = NamedTempFile::new("testfile.txt")?;
		let invalid_path = NamedTempFile::new("nonexistent.txt")?;

		// Create a temporary file
		{
			let mut temp_file = File::create(&temp_file_path)?;
			temp_file.write_all(b"Hello, world!")?;
		}

		// Test with a valid file path
		let data_reader_file = DataReaderFile::open(&temp_file_path);
		assert!(data_reader_file.is_ok());

		// Test with an invalid file path
		let data_reader_file = DataReaderFile::open(&invalid_path);
		assert!(data_reader_file.is_err());

		Ok(())
	}

	// Test the 'read_range' method
	#[tokio::test]
	async fn read_range() -> Result<()> {
		let temp_file_path = NamedTempFile::new("testfile.txt")?;

		// Create a temporary file
		{
			let mut temp_file = File::create(&temp_file_path)?;
			temp_file.write_all(b"Hello, world!")?;
		}

		let mut data_reader_file = DataReaderFile::open(&temp_file_path)?;

		// Define a range to read
		let range = ByteRange { offset: 4, length: 6 };

		// Read the specified range from the file
		let blob = data_reader_file.read_range(&range).await?;

		// Check if the read range matches the expected text
		assert_eq!(blob.as_str(), "o, wor");

		Ok(())
	}

	// Test the 'get_name' method
	#[tokio::test]
	async fn get_name() -> Result<()> {
		let temp_file_path = NamedTempFile::new("testfile.txt")?;

		// Create a temporary file
		{
			let mut temp_file = File::create(&temp_file_path)?;
			temp_file.write_all(b"Hello, world!")?;
		}

		let data_reader_file = DataReaderFile::open(&temp_file_path)?;

		// Check if the name matches the original file path
		assert_wildcard!(
			data_reader_file.get_name(),
			&format!("*{}", temp_file_path.to_str().unwrap())
		);

		Ok(())
	}
}
