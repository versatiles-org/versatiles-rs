use crate::{
	server::{guess_mime, make_result, ServerSourceResult, ServerSourceTrait},
	shared::{Blob, Compression, Result, TargetCompression},
};
use async_trait::async_trait;
use std::{
	env::current_dir,
	fmt::Debug,
	fs::File,
	io::{BufReader, Read},
	path::{Path, PathBuf},
};

// Folder struct definition
pub struct Folder {
	folder: PathBuf,
	name: String,
}

impl Folder {
	// Constructor for the Folder struct
	pub fn from(path: &str) -> Result<Box<Folder>> {
		let mut folder = current_dir()?;
		folder.push(Path::new(path));

		// Check that the folder exists, is absolute and is a directory
		assert!(folder.exists(), "path {folder:?} does not exist");
		assert!(folder.is_absolute(), "path {folder:?} must be absolute");
		assert!(folder.is_dir(), "path {folder:?} must be a directory");

		folder = folder.canonicalize()?;

		// Create a new Folder struct with the given path and name
		Ok(Box::new(Folder {
			folder,
			name: path.to_string(),
		}))
	}
}

#[async_trait]
impl ServerSourceTrait for Folder {
	// Returns the name of the folder
	fn get_name(&self) -> Result<String> {
		Ok(self.name.clone())
	}

	// Returns a JSON string containing the folder's type
	fn get_info_as_json(&self) -> Result<String> {
		Ok("{\"type\":\"folder\"}".to_string())
	}

	// Gets the data at the given path and responds with a compressed or uncompressed version
	// based on the accept header
	async fn get_data(&mut self, path: &[&str], _accept: &TargetCompression) -> Option<ServerSourceResult> {
		let mut local_path = self.folder.clone();
		local_path.push(PathBuf::from(path.join("/")));

		// If the path is a directory, append 'index.html'
		if local_path.is_dir() {
			local_path.push("index.html");
		}

		// If the local path is not a subpath of the folder or it doesn't exist, return not found
		if !local_path.starts_with(&self.folder) || !local_path.exists() || !local_path.is_file() {
			return None;
		}

		let f = File::open(&local_path).unwrap();
		let mut buffer = Vec::new();
		BufReader::new(f).read_to_end(&mut buffer).unwrap();
		let blob = Blob::from(buffer);

		let mime = guess_mime(&local_path);

		return make_result(blob, &Compression::None, &mime);
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
	use super::Folder;
	use crate::{server::ServerSourceTrait, shared::TargetCompression};

	#[tokio::test]
	async fn test() {
		// Create a new Folder instance
		let mut folder = Folder::from("testdata").unwrap();

		assert_eq!(
			format!("{:?}", folder),
			"Folder { folder: \"/Users/michaelkreil/Projekte/versatiles/versatiles-rs/testdata\", name: \"testdata\" }"
		);

		// Test get_name function
		assert_eq!(folder.get_name().unwrap(), "testdata");

		// Test get_info_as_json function
		assert_eq!(folder.get_info_as_json().unwrap(), "{\"type\":\"folder\"}");

		// Test get_data function with a non-existent file
		let result = folder
			.get_data(&["recipes", "Queijo.txt"], &TargetCompression::from_none())
			.await;
		assert!(result.is_none());

		// Test get_data function with an existing file
		let result = folder
			.get_data(&["berlin.mbtiles"], &TargetCompression::from_none())
			.await;
		assert!(result.is_some());

		let result = result.unwrap().blob;
		assert_eq!(result.len(), 26533888);
	}
}
