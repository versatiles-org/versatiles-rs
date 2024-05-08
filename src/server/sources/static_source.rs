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
			source: Arc::new(if std::fs::metadata(path)?.is_dir() {
				Box::new(Folder::from(path)?)
			} else {
				Box::new(TarFile::from(path)?)
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
	use std::{fs::File, io::Write, path::PathBuf};
	use versatiles_lib::shared::{compress, Blob, Compression};

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

	#[test]
	fn new_static_source() -> Result<()> {
		let check_type = |path: PathBuf, type_name: &str| {
			let source = StaticSource::new(&path, Url::new("")).unwrap();
			assert_eq!(source.get_type(), type_name);
		};

		let check_error = |path: PathBuf, error_should: &str| {
			let source = StaticSource::new(&path, Url::new(""));
			let error = source.err().unwrap().to_string();
			assert!(
				error.ends_with(error_should),
				"{} must ends_with {}",
				error,
				error_should
			);
		};

		let create_file = |path: &PathBuf, compression: Compression| {
			let content = compress(Blob::new_empty(), &compression).unwrap();
			let mut f = File::create(path).unwrap();
			f.write_all(content.as_slice()).unwrap();
		};

		// Create temporary file and directory for testing
		let temp_dir = assert_fs::TempDir::new()?;

		// Test non existent file
		let path = temp_dir.path().join("non_existent.tar");
		check_error(path, "No such file or directory (os error 2)");

		// Test .tar file
		let path = temp_dir.path().join("temp.tar");
		create_file(&path, Compression::None);
		check_type(path, "tar");

		// Test gzip compressed .tar file
		let path = temp_dir.path().join("temp.tar.gz");
		create_file(&path, Compression::Gzip);
		check_type(path, "tar");

		// Test brotli compressed .tar file
		let path = temp_dir.path().join("temp.tar.br");
		create_file(&path, Compression::Brotli);
		check_type(path, "tar");

		// Test non .tar file
		let path = temp_dir.path().join("data.tar.bmp");
		create_file(&path, Compression::None);
		check_error(path, "\" must be a name of a tar file");

		// Test initialization with a folder
		let path = temp_dir.path().join("folder");
		std::fs::create_dir(&path)?;
		check_type(path, "folder");

		Ok(())
	}

	#[tokio::test]
	async fn get_data_valid_path() {
		let static_source = StaticSource {
			source: Arc::new(Box::new(MockStaticSource)),
			prefix: Url::new(""),
		};
		let result = static_source.get_data(&Url::new("exists"), &TargetCompression::from_none());
		assert!(result.is_some());
	}

	#[tokio::test]
	async fn get_data_invalid_path() {
		let static_source = StaticSource {
			source: Arc::new(Box::new(MockStaticSource)),
			prefix: Url::new(""),
		};
		let result = static_source.get_data(&Url::new("does_not_exist"), &TargetCompression::from_none());
		assert!(result.is_none());
	}

	#[tokio::test]
	async fn get_data_with_path_filtering() {
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
