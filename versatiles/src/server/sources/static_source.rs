use super::{response::SourceResponse, static_source_folder::Folder, static_source_tar::TarFile};
use anyhow::Result;
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_lib::shared::TargetCompression;

#[async_trait]
pub trait StaticSourceTrait: Send + Sync + Debug {
	#[cfg(test)]
	fn get_type(&self) -> String;
	#[cfg(test)]
	fn get_name(&self) -> Result<String>;
	fn get_data(&self, path: &[&str], accept: &TargetCompression) -> Option<SourceResponse>;
}

#[derive(Clone)]
pub struct StaticSource {
	source: Arc<Box<dyn StaticSourceTrait>>,
	path: Vec<String>,
}

impl StaticSource {
	pub fn new(filename: &str, uncleaned_path: &str) -> Result<StaticSource> {
		let mut path: Vec<String> = uncleaned_path.trim().split('/').map(|s| s.to_string()).collect();
		while path.first() == Some(&String::from("")) {
			path.remove(0);
		}
		while path.last() == Some(&String::from("")) {
			path.pop();
		}

		Ok(StaticSource {
			source: Arc::new(if filename.ends_with(".tar") {
				Box::new(TarFile::from(filename)?)
			} else {
				Box::new(Folder::from(filename)?)
			}),
			path,
		})
	}
	#[cfg(test)]
	pub fn get_type(&self) -> String {
		self.source.get_type()
	}
	pub fn get_data(&self, path: &[&str], accept: &TargetCompression) -> Option<SourceResponse> {
		if self.path.is_empty() {
			self.source.get_data(path, accept)
		} else {
			let mut path_vec: Vec<&str> = path.to_vec();
			for segment in self.path.iter() {
				if path_vec.is_empty() || (segment != path_vec[0]) {
					return None;
				}
				path_vec.remove(0);
			}
			self.source.get_data(path_vec.as_slice(), accept)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Ok;
	use async_trait::async_trait;
	use versatiles_lib::shared::{Blob, Compression};

	#[derive(Debug)]
	struct MockStaticSource;

	#[async_trait]
	impl StaticSourceTrait for MockStaticSource {
		fn get_type(&self) -> String {
			String::from("mock")
		}

		fn get_name(&self) -> Result<String> {
			Ok("MockSource".into())
		}

		fn get_data(&self, path: &[&str], _accept: &TargetCompression) -> Option<SourceResponse> {
			if path.contains(&"exists") {
				SourceResponse::new_some(Blob::from(vec![1, 2, 3, 4]), &Compression::None, "application/octet-stream")
			} else {
				None
			}
		}
	}

	#[tokio::test]
	async fn test_static_source_new_integration() {
		// Create temporary file and directory for testing
		let temp_dir = assert_fs::TempDir::new().unwrap();
		let temp_file_path = temp_dir.path().join("temp.tar");
		let temp_folder_path = temp_dir.path().join("folder");
		std::fs::create_dir(&temp_folder_path).unwrap();
		std::fs::File::create(&temp_file_path).unwrap();

		// Test initialization with a .tar file
		let tar_source = StaticSource::new(temp_file_path.to_str().unwrap(), "").unwrap();
		assert_eq!(tar_source.get_type(), "tar");

		// Test initialization with a folder
		let folder_source = StaticSource::new(temp_folder_path.to_str().unwrap(), "").unwrap();
		assert_eq!(folder_source.get_type(), "folder");
	}

	#[tokio::test]
	async fn test_get_data_valid_path() {
		let static_source = StaticSource {
			source: Arc::new(Box::new(MockStaticSource)),
			path: vec![],
		};
		let result = static_source.get_data(&["exists"], &TargetCompression::from_none());
		assert!(result.is_some());
	}

	#[tokio::test]
	async fn test_get_data_invalid_path() {
		let static_source = StaticSource {
			source: Arc::new(Box::new(MockStaticSource)),
			path: vec![],
		};
		let result = static_source.get_data(&["does_not_exist"], &TargetCompression::from_none());
		assert!(result.is_none());
	}

	#[tokio::test]
	async fn test_get_data_with_path_filtering() {
		let static_source = StaticSource {
			source: Arc::new(Box::new(MockStaticSource)),
			path: vec!["path".into(), "to".into()],
		};
		// Should match and retrieve data
		let result = static_source.get_data(&["path", "to", "exists"], &TargetCompression::from_none());
		assert!(result.is_some());

		// Should fail due to path mismatch
		let result = static_source.get_data(&["path", "wrong", "exists"], &TargetCompression::from_none());
		assert!(result.is_none());
	}
}
