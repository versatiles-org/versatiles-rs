use super::{
	super::utils::{Url, guess_mime},
	SourceResponse,
	static_source::StaticSourceTrait,
};
use async_trait::async_trait;
use reqwest::Url as ReqwestUrl;
use std::{fmt::Debug, path::Path};
use versatiles_core::{
	TileCompression,
	compression::TargetCompression,
	io::{DataReaderHttp, DataReaderTrait},
};

pub struct RemoteFolder {
	base_url: ReqwestUrl,
	name: String,
}

impl RemoteFolder {
	pub fn from(url: &ReqwestUrl) -> Self {
		let mut base_url = url.clone();
		// Ensure the base URL ends with '/' so that relative joins append rather than replace
		if !base_url.path().ends_with('/') {
			base_url.set_path(&format!("{}/", base_url.path()));
		}
		RemoteFolder {
			name: base_url.to_string(),
			base_url,
		}
	}
}

#[async_trait]
impl StaticSourceTrait for RemoteFolder {
	#[cfg(test)]
	fn type_name(&self) -> &str {
		"remote_folder"
	}

	#[cfg(test)]
	fn name(&self) -> &str {
		&self.name
	}

	async fn get_data(&self, url: &Url, _accept: &TargetCompression) -> Option<SourceResponse> {
		let path = url.str.trim_start_matches('/');
		let target_url = self.base_url.join(path).ok()?;
		let reader = DataReaderHttp::try_from(&target_url).ok()?;
		let blob = reader.read_all().await.ok()?;
		let mime = guess_mime(Path::new(target_url.path()));
		SourceResponse::new_some(blob, TileCompression::Uncompressed, &mime)
	}
}

impl Debug for RemoteFolder {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("RemoteFolder").field("base_url", &self.name).finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn base_url_gets_trailing_slash() {
		let url = ReqwestUrl::parse("https://example.com/assets").unwrap();
		let folder = RemoteFolder::from(&url);
		assert!(folder.base_url.path().ends_with('/'), "base_url must end with '/'");
		assert_eq!(folder.base_url.as_str(), "https://example.com/assets/");
	}

	#[test]
	fn base_url_keeps_existing_trailing_slash() {
		let url = ReqwestUrl::parse("https://example.com/assets/").unwrap();
		let folder = RemoteFolder::from(&url);
		assert_eq!(folder.base_url.as_str(), "https://example.com/assets/");
	}

	#[test]
	fn debug_impl() {
		let url = ReqwestUrl::parse("https://example.com/assets/").unwrap();
		let folder = RemoteFolder::from(&url);
		let debug = format!("{folder:?}");
		assert!(debug.contains("RemoteFolder"));
		assert!(debug.contains("https://example.com/assets/"));
	}
}
