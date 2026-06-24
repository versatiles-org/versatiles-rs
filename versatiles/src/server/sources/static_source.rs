use super::{
	super::utils::Url, SourceResponse, static_source_folder::Folder, static_source_remote_folder::RemoteFolder,
	static_source_tar::TarFile,
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::DataLocation;
use versatiles_core::compression::TargetCompression;
use versatiles_derive::context;

#[async_trait]
pub trait StaticSourceTrait: Send + Sync + Debug {
	#[cfg(test)]
	fn type_name(&self) -> &str;
	#[cfg(test)]
	fn name(&self) -> &str;
	async fn get_data(&self, url: &Url, accept: &TargetCompression) -> Option<SourceResponse>;
}

#[derive(Clone)]
pub struct StaticSource {
	source: Arc<Box<dyn StaticSourceTrait>>,
	prefix: Url,
}

impl StaticSource {
	#[context("creating static source from location: location={location:?}, prefix={prefix}")]
	pub async fn from_location(location: &DataLocation, prefix: &str) -> Result<StaticSource> {
		let prefix = Url::from(prefix).to_dir();
		Ok(StaticSource {
			source: Arc::new(match location {
				DataLocation::Url(url) => {
					let filename = url.path_segments().and_then(|mut s| s.next_back()).unwrap_or("");
					if filename.contains(".tar") {
						Box::new(TarFile::from_url(url).await?) as Box<dyn StaticSourceTrait>
					} else {
						Box::new(RemoteFolder::from(url)) as Box<dyn StaticSourceTrait>
					}
				}
				DataLocation::Path(path) => {
					if std::fs::metadata(path)?.is_dir() {
						Box::new(Folder::from(path)?) as Box<dyn StaticSourceTrait>
					} else {
						Box::new(TarFile::from(path)?)
					}
				}
				DataLocation::Blob(_) => bail!("Blob is not supported as a static source"),
			}),
			prefix,
		})
	}

	#[cfg(test)]
	pub fn type_name(&self) -> &str {
		self.source.type_name()
	}

	pub fn prefix(&self) -> &Url {
		&self.prefix
	}

	pub async fn get_data(&self, url: &Url, accept: &TargetCompression) -> Option<SourceResponse> {
		if !url.starts_with(&self.prefix) {
			return None;
		}
		self
			.source
			.get_data(
				&url.strip_prefix(&self.prefix).expect("prefix match checked above"),
				accept,
			)
			.await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use async_trait::async_trait;
	use std::{fs::File, io::Write, path::PathBuf};
	use versatiles_core::{Blob, TileCompression, compression::compress};

	#[derive(Debug)]
	struct MockStaticSource;

	#[async_trait]
	impl StaticSourceTrait for MockStaticSource {
		fn type_name(&self) -> &str {
			"mock"
		}

		fn name(&self) -> &str {
			"MockSource"
		}

		async fn get_data(&self, path: &Url, _accept: &TargetCompression) -> Option<SourceResponse> {
			if path.starts_with(&Url::from("exists")) {
				SourceResponse::new_some(
					Blob::from(vec![1, 2, 3, 4]),
					TileCompression::Uncompressed,
					"application/octet-stream",
				)
			} else {
				None
			}
		}
	}

	#[tokio::test]
	async fn from_location_static_source() -> Result<()> {
		use TileCompression::*;
		use versatiles_container::DataLocation;

		let check_type = |path: PathBuf, type_name: &'static str| async move {
			let loc = DataLocation::from(path);
			let source = StaticSource::from_location(&loc, "").await.unwrap();
			assert_eq!(source.type_name(), type_name);
		};

		let check_error = |path: PathBuf, error_should: &'static str| async move {
			let loc = DataLocation::from(path);
			let result = StaticSource::from_location(&loc, "").await;
			let error = result
				.err()
				.iter()
				.flat_map(|e| e.chain().map(std::string::ToString::to_string))
				.collect::<Vec<_>>()
				.join(" -> ");
			assert!(
				error.ends_with(error_should),
				"Error message '{error}' must end with '{error_should}'"
			);
		};

		let create_file = |path: &PathBuf, compression: TileCompression| {
			let content = compress(Blob::new_empty(), &compression).unwrap();
			let mut f = File::create(path).unwrap();
			f.write_all(content.as_slice()).unwrap();
		};

		// Create temporary file and directory for testing
		let temp_dir = assert_fs::TempDir::new()?;

		// Test non existent file
		let path = temp_dir.path().join("non_existent.tar");
		check_error(path, "(os error 2)").await;

		// Test .tar file
		let path = temp_dir.path().join("temp.tar");
		create_file(&path, Uncompressed);
		check_type(path, "tar").await;

		// Test gzip compressed .tar file
		let path = temp_dir.path().join("temp.tar.gz");
		create_file(&path, Gzip);
		check_type(path, "tar").await;

		// Test brotli compressed .tar file
		let path = temp_dir.path().join("temp.tar.br");
		create_file(&path, Brotli);
		check_type(path, "tar").await;

		// Test non .tar file — treated as remote folder (URL), but path is local so error differs
		let path = temp_dir.path().join("data.tar.bmp");
		create_file(&path, Uncompressed);
		check_error(path, "\" must be a name of a tar file").await;

		// Test initialization with a folder
		let path = temp_dir.path().join("folder");
		std::fs::create_dir(&path)?;
		check_type(path, "folder").await;

		Ok(())
	}

	#[tokio::test]
	async fn get_data_valid_path() {
		let static_source = StaticSource {
			source: Arc::new(Box::new(MockStaticSource)),
			prefix: Url::from(""),
		};
		let result = static_source
			.get_data(&Url::from("exists"), &TargetCompression::from_none())
			.await;
		assert!(result.is_some());
	}

	#[tokio::test]
	async fn get_data_invalid_path() {
		let static_source = StaticSource {
			source: Arc::new(Box::new(MockStaticSource)),
			prefix: Url::from(""),
		};
		let result = static_source
			.get_data(&Url::from("does_not_exist"), &TargetCompression::from_none())
			.await;
		assert!(result.is_none());
	}

	#[tokio::test]
	async fn get_data_with_path_filtering() {
		let static_source = StaticSource {
			source: Arc::new(Box::new(MockStaticSource)),
			prefix: Url::from("path/to"),
		};
		let result = static_source
			.get_data(&Url::from("path/to/exists"), &TargetCompression::from_none())
			.await;
		assert!(result.is_some());

		let result = static_source
			.get_data(&Url::from("path/wrong/exists"), &TargetCompression::from_none())
			.await;
		assert!(result.is_none());
	}
}
