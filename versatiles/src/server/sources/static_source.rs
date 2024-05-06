use super::{static_source_folder::Folder, static_source_tar::TarFile, SourceResponse};
use crate::server::helpers::Url;
use anyhow::{ensure, Result};
use async_trait::async_trait;
use std::{fmt::Debug, path::Path, sync::Arc};
use versatiles_lib::shared::TargetCompression;

#[async_trait]
pub trait StaticSourceTrait: Send + Sync + Debug {
	#[cfg(test)]
	fn get_type(&self) -> &str;
	#[cfg(test)]
	fn get_name(&self) -> &str;
	fn get_data(&self, url: &Url, accept: &TargetCompression) -> Option<SourceResponse>;
}

#[derive(Clone)]
pub struct StaticSource {
	source: Arc<Box<dyn StaticSourceTrait>>,
	prefix: Url,
}

impl StaticSource {
	pub fn new(path: &Path, prefix: Url) -> Result<StaticSource> {
		ensure!(prefix.is_dir());

		Ok(StaticSource {
			source: Arc::new(if std::fs::metadata(path).unwrap().is_dir() {
				Box::new(Folder::from(path).unwrap())
			} else {
				Box::new(TarFile::from(path).unwrap())
			}),
			prefix,
		})
	}
	#[cfg(test)]
	pub fn get_type(&self) -> &str {
		self.source.get_type()
	}
	pub fn get_data(&self, url: &Url, accept: &TargetCompression) -> Option<SourceResponse> {
		if !url.starts_with(&self.prefix) {
			return None;
		}
		self.source.get_data(&url.strip_prefix(&self.prefix).unwrap(), accept)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use async_trait::async_trait;
	use versatiles_lib::shared::{Blob, Compression};

	#[derive(Debug)]
	struct MockStaticSource;

	#[async_trait]
	impl StaticSourceTrait for MockStaticSource {
		fn get_type(&self) -> &str {
			"mock"
		}

		fn get_name(&self) -> &str {
			"MockSource"
		}

		fn get_data(&self, path: &Url, _accept: &TargetCompression) -> Option<SourceResponse> {
			if path.starts_with(&Url::new("exists")) {
				SourceResponse::new_some(
					Blob::from(vec![1, 2, 3, 4]),
					&Compression::None,
					"application/octet-stream",
				)
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
		let tar_source = StaticSource::new(&temp_file_path, Url::new("")).unwrap();
		assert_eq!(tar_source.get_type(), "tar");

		// Test initialization with a folder
		let folder_source = StaticSource::new(&temp_folder_path, Url::new("")).unwrap();
		assert_eq!(folder_source.get_type(), "folder");
	}

	#[tokio::test]
	async fn test_get_data_valid_path() {
		let static_source = StaticSource {
			source: Arc::new(Box::new(MockStaticSource)),
			prefix: Url::new(""),
		};
		let result = static_source.get_data(&Url::new("exists"), &TargetCompression::from_none());
		assert!(result.is_some());
	}

	#[tokio::test]
	async fn test_get_data_invalid_path() {
		let static_source = StaticSource {
			source: Arc::new(Box::new(MockStaticSource)),
			prefix: Url::new(""),
		};
		let result = static_source.get_data(&Url::new("does_not_exist"), &TargetCompression::from_none());
		assert!(result.is_none());
	}

	#[tokio::test]
	async fn test_get_data_with_path_filtering() {
		let static_source = StaticSource {
			source: Arc::new(Box::new(MockStaticSource)),
			prefix: Url::new("path/to"),
		};
		// Should match and retrieve data
		let result = static_source.get_data(&Url::new("path/to/exists"), &TargetCompression::from_none());
		assert!(result.is_some());

		// Should fail due to path mismatch
		let result = static_source.get_data(&Url::new("path/wrong/exists"), &TargetCompression::from_none());
		assert!(result.is_none());
	}
}
