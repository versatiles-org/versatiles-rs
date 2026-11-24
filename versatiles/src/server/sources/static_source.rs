use std::{fmt::Debug, sync::Arc};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use axum::http::HeaderValue;
use versatiles_container::DataLocation;
use versatiles_core::compression::TargetCompression;
use versatiles_derive::context;

use super::{
	super::utils::Url, SourceResponse, static_source_folder::Folder, static_source_remote_folder::RemoteFolder,
	static_source_tar::TarFile,
};
use crate::config::{IMMUTABLE_CACHE_CONTROL, PathPattern, StaticSourceConfig};

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
	source: Arc<dyn StaticSourceTrait>,
	prefix: Url,
	/// `Cache-Control` sent with the files of this source that no `immutable`
	/// pattern matches.
	cache_control: Arc<str>,
	/// Patterns of versioned paths inside the source, sent with
	/// [`IMMUTABLE_CACHE_CONTROL`].
	immutable: Arc<[PathPattern]>,
}

impl StaticSource {
	/// Open the files at `location`, to be served under `prefix` with
	/// `cache_control`, or with [`IMMUTABLE_CACHE_CONTROL`] where one of the
	/// `immutable` patterns matches.
	///
	/// A `cache_control` that is no valid header value is refused here, so that
	/// it fails when the source is mounted, not on its first request.
	#[context("creating static source from location: location={location:?}, prefix={prefix}")]
	pub async fn from_location(
		location: &DataLocation,
		prefix: &str,
		follow_symlinks: bool,
		cache_control: &str,
		immutable: Vec<PathPattern>,
	) -> Result<StaticSource> {
		HeaderValue::from_str(cache_control)
			.map_err(|e| anyhow!("invalid cache_control value {cache_control:?}: {e}"))?;
		let prefix = Url::from(prefix).to_dir();
		Ok(StaticSource {
			source: match location {
				DataLocation::Url(url) => {
					let filename = url.path_segments().and_then(|mut s| s.next_back()).unwrap_or("");
					if filename.contains(".tar") {
						Arc::new(TarFile::from_url(url).await?) as Arc<dyn StaticSourceTrait>
					} else {
						Arc::new(RemoteFolder::from(url)) as Arc<dyn StaticSourceTrait>
					}
				}
				DataLocation::Path(path) => {
					if std::fs::metadata(path)?.is_dir() {
						Arc::new(Folder::from(path, follow_symlinks)?) as Arc<dyn StaticSourceTrait>
					} else {
						Arc::new(TarFile::from(path).await?)
					}
				}
				DataLocation::Blob(_) => bail!("Blob is not supported as a static source"),
			},
			prefix,
			cache_control: Arc::from(cache_control),
			immutable: Arc::from(immutable),
		})
	}

	/// Open the files a `static` entry of the config names, with its `prefix`
	/// (default `/`), `cache_control` and `immutable` patterns.
	pub async fn from_config(config: &StaticSourceConfig, follow_symlinks: bool) -> Result<StaticSource> {
		StaticSource::from_location(
			&config.src,
			config.prefix.as_deref().unwrap_or("/"),
			follow_symlinks,
			config.cache_control(),
			config.immutable_patterns()?,
		)
		.await
	}

	#[cfg(test)]
	pub fn type_name(&self) -> &str {
		self.source.type_name()
	}

	pub fn prefix(&self) -> &Url {
		&self.prefix
	}

	/// The `Cache-Control` header sent with this source's files that no
	/// `immutable` pattern matches.
	#[cfg(test)]
	pub fn cache_control(&self) -> &str {
		&self.cache_control
	}

	/// The `Cache-Control` header for the file at `url`, a request path under
	/// this source's prefix.
	///
	/// The patterns are matched against the path without the prefix, decoded
	/// segment by segment, so a pattern from a bundle's documentation works
	/// wherever the bundle is mounted.
	pub fn cache_control_for(&self, url: &Url) -> &str {
		if !self.immutable.is_empty()
			&& let Ok(path) = url.strip_prefix(&self.prefix)
			&& let Some(segments) = path.decoded_segments()
			&& self.immutable.iter().any(|pattern| pattern.matches(&segments))
		{
			return IMMUTABLE_CACHE_CONTROL;
		}
		&self.cache_control
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
	use std::{fs::File, io::Write, path::PathBuf};

	use async_trait::async_trait;
	use versatiles_core::{Blob, TileCompression, compression::compress};

	use super::*;

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
			let source = StaticSource::from_location(&loc, "", false, "no-cache", Vec::new())
				.await
				.unwrap();
			assert_eq!(source.type_name(), type_name);
		};

		let check_error = |path: PathBuf, error_should: &'static str| async move {
			let loc = DataLocation::from(path);
			let result = StaticSource::from_location(&loc, "", false, "no-cache", Vec::new()).await;
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

		// Test zstd compressed .tar file
		let path = temp_dir.path().join("temp.tar.zst");
		create_file(&path, Zstd);
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
			source: Arc::new(MockStaticSource),
			prefix: Url::from(""),
			cache_control: Arc::from("no-cache"),
			immutable: Arc::from(Vec::new()),
		};
		let result = static_source
			.get_data(&Url::from("exists"), &TargetCompression::from_none())
			.await;
		assert!(result.is_some());
	}

	#[tokio::test]
	async fn get_data_invalid_path() {
		let static_source = StaticSource {
			source: Arc::new(MockStaticSource),
			prefix: Url::from(""),
			cache_control: Arc::from("no-cache"),
			immutable: Arc::from(Vec::new()),
		};
		let result = static_source
			.get_data(&Url::from("does_not_exist"), &TargetCompression::from_none())
			.await;
		assert!(result.is_none());
	}

	#[tokio::test]
	async fn get_data_with_path_filtering() {
		let static_source = StaticSource {
			source: Arc::new(MockStaticSource),
			prefix: Url::from("path/to"),
			cache_control: Arc::from("no-cache"),
			immutable: Arc::from(Vec::new()),
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

	#[tokio::test]
	async fn from_location_refuses_an_invalid_cache_control() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let loc = DataLocation::from(temp_dir.path().to_path_buf());
		let error = StaticSource::from_location(&loc, "", false, "no-cache\n", Vec::new())
			.await
			.err()
			.expect("a header value with a newline is refused");
		assert!(
			format!("{error:#}").contains("invalid cache_control value"),
			"{error:#}"
		);

		let source = StaticSource::from_location(&loc, "", false, "public, max-age=60", Vec::new()).await?;
		assert_eq!(source.cache_control(), "public, max-age=60");
		Ok(())
	}

	#[test]
	fn immutable_patterns_are_matched_without_the_prefix() {
		let static_source = StaticSource {
			source: Arc::new(MockStaticSource),
			prefix: Url::from("/map/").to_dir(),
			cache_control: Arc::from("no-cache"),
			immutable: Arc::from(vec![PathPattern::parse("assets/lib/*@*/**").unwrap()]),
		};
		let header = |path: &str| static_source.cache_control_for(&Url::from(path)).to_string();

		assert_eq!(
			header("/map/assets/lib/maplibre-gl@5.6.0/maplibre-gl.js"),
			IMMUTABLE_CACHE_CONTROL
		);
		// Requests arrive percent-encoded; the patterns see the decoded names.
		assert_eq!(
			header("/map/assets/lib/maplibre-gl%405.6.0/maplibre-gl.js"),
			IMMUTABLE_CACHE_CONTROL
		);
		assert_eq!(header("/map/assets/lib/maplibre-gl/maplibre-gl.js"), "no-cache");
		assert_eq!(header("/map/index.html"), "no-cache");
	}
}
