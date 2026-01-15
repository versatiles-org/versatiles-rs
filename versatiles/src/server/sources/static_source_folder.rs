use super::{
	super::{Url, utils::guess_mime},
	SourceResponse,
	static_source::StaticSourceTrait,
};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use std::{
	env::current_dir,
	fmt::Debug,
	fs::File,
	io::{BufReader, Read},
	path::{Path, PathBuf},
};
use versatiles_core::{Blob, TileCompression, compression::TargetCompression};
use versatiles_derive::context;

// Folder struct definition
#[derive(Clone)]
pub struct Folder {
	folder: PathBuf,
	name: String,
}

impl Folder {
	// Constructor for the Folder struct
	#[context("loading static folder from path: {path:?}")]
	pub fn from(path: &Path) -> Result<Folder> {
		let mut folder = current_dir()?;
		folder.push(Path::new(path));
		folder = folder.canonicalize()?;

		// Check that the folder exists, is absolute and is a directory
		ensure!(folder.exists(), "path {folder:?} does not exist");
		ensure!(folder.is_absolute(), "path {folder:?} must be absolute");
		ensure!(folder.is_dir(), "path {folder:?} must be a directory");

		folder = folder.canonicalize()?;

		// Create a new Folder struct with the given path and name
		Ok(Folder {
			folder,
			name: path.to_str().unwrap().to_owned(),
		})
	}
}

#[async_trait]
impl StaticSourceTrait for Folder {
	#[cfg(test)]
	fn get_type(&self) -> &str {
		"folder"
	}

	// Returns the name of the folder
	#[cfg(test)]
	fn get_name(&self) -> &str {
		&self.name
	}

	// Gets the data at the given path and responds with a compressed or uncompressed version
	// based on the accept header
	fn get_data(&self, url: &Url, _accept: &TargetCompression) -> Option<SourceResponse> {
		let mut local_path = url.to_pathbug(&self.folder);

		// If the path is a directory, append 'index.html'
		if local_path.is_dir() {
			local_path.push("index.html");
		}

		// If the local path is not a subpath of the folder, return not found
		if !local_path.starts_with(&self.folder) {
			return None;
		}

		let mime = guess_mime(&local_path);

		// Check for compressed versions first (".br" and ".gz"), falling back to uncompressed if neither is found

		let (file, compression) = if let Ok(file) = File::open(&local_path) {
			(file, TileCompression::Uncompressed)
		} else if let Ok(file) = File::open(format!("{}.br", local_path.display())) {
			(file, TileCompression::Brotli)
		} else if let Ok(file) = File::open(format!("{}.gz", local_path.display())) {
			(file, TileCompression::Gzip)
		} else {
			return None;
		};

		let mut buffer = Vec::new();
		BufReader::new(file).read_to_end(&mut buffer).unwrap();

		SourceResponse::new_some(Blob::from(buffer), compression, &mime)
	}
}

impl Debug for Folder {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Folder")
			.field("folder", &self.folder)
			.field("name", &self.name)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test() {
		// Create a new Folder instance
		let folder = Folder::from(Path::new("../testdata")).unwrap();

		let debug: String = format!("{folder:?}");
		assert!(debug.starts_with("Folder { folder: \""));
		assert!(debug.ends_with("testdata\", name: \"../testdata\" }"));

		// Test get_name function
		assert_eq!(folder.get_name(), "../testdata");

		// Test get_data function with a non-existent file
		let result = folder.get_data(&Url::from("recipes/Queijo.txt"), &TargetCompression::from_none());
		assert!(result.is_none());

		// Test get_data function with an existing uncompressed file
		let result = folder.get_data(&Url::from("berlin.mbtiles"), &TargetCompression::from_none());
		assert!(result.is_some());

		let result = result.unwrap();
		assert_eq!(result.blob.len(), 26533888);
		assert_eq!(result.compression, TileCompression::Uncompressed);
	}

	#[tokio::test]
	async fn directory_with_index_html() {
		// Setup: Create a temporary directory and place an index.html file inside it
		let temp_dir = assert_fs::TempDir::new().unwrap();
		let dir_path = temp_dir.path().join("testdir");
		std::fs::create_dir(&dir_path).unwrap_or_default();

		let index_path = dir_path.join("index.html");
		std::fs::write(&index_path, b"Hello, world!").unwrap();

		// Test initialization with the temporary directory
		let folder = Folder::from(temp_dir.path()).unwrap();

		// Attempt to retrieve data from the directory, expecting to get the contents of index.html
		let response = folder
			.get_data(&Url::from("testdir"), &TargetCompression::from_none())
			.unwrap();

		let result = response.blob.as_str();
		assert_eq!(result, "Hello, world!");
		assert_eq!(response.compression, TileCompression::Uncompressed);
	}

	#[tokio::test]
	async fn test_compressed_files() {
		// Setup: Create a temporary directory with Brotli and Gzip compressed files
		let temp_dir = assert_fs::TempDir::new().unwrap();
		let file_path = temp_dir.path().join("compressed.txt");

		// Create Brotli-compressed file
		let br_file_path = file_path.with_extension("txt.br");
		std::fs::write(&br_file_path, b"Brotli compressed content").unwrap();

		// Create Gzip-compressed file
		let gz_file_path = file_path.with_extension("txt.gz");
		std::fs::write(&gz_file_path, b"Gzip compressed content").unwrap();

		// Initialize folder and test get_data with Brotli file
		let folder = Folder::from(temp_dir.path()).unwrap();

		// Test Brotli compression
		let response_br = folder
			.get_data(&Url::from("compressed.txt"), &TargetCompression::from_none())
			.unwrap();

		assert_eq!(response_br.blob.as_str(), "Brotli compressed content");
		assert_eq!(response_br.compression, TileCompression::Brotli);

		// Remove Brotli file to test Gzip fallback
		std::fs::remove_file(&br_file_path).unwrap();

		// Test Gzip compression
		let response_gz = folder
			.get_data(&Url::from("compressed.txt"), &TargetCompression::from_none())
			.unwrap();

		assert_eq!(response_gz.blob.as_str(), "Gzip compressed content");
		assert_eq!(response_gz.compression, TileCompression::Gzip);

		// Cleanup
		temp_dir.close().unwrap();
	}
}
