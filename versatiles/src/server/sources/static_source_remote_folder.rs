use std::{fmt::Debug, path::Path};

use async_trait::async_trait;
use percent_encoding::percent_decode_str;
use reqwest::Url as ReqwestUrl;
use versatiles_core::{
	TileCompression,
	compression::TargetCompression,
	io::{DataReaderHttp, DataReaderTrait},
};

use super::{
	super::utils::{Url, guess_mime},
	SourceResponse,
	static_source::StaticSourceTrait,
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

	/// Resolves a request path against the configured base URL.
	///
	/// Deliberately *not* `Url::join`. `join` implements the WHATWG rules, where a
	/// segment carrying a scheme is an absolute URL: joining `http:/10.0.0.1/x`
	/// onto an `https://` base yields `http://10.0.0.1/x`, a different host
	/// entirely, which turns any request into a fetch of the server's choosing.
	/// `path_segments_mut` cannot reach the scheme or the host — it edits the path
	/// of an already-parsed URL — so the escape is closed by construction rather
	/// than by spotting the segments that trigger it.
	///
	/// Segments arrive percent-encoded, so each is decoded before it is inspected
	/// and re-encoded by `extend`. Decoding first is what makes the `.`/`..` check
	/// meaningful: `join` treats `%2e%2e` as a dot segment, so comparing the raw
	/// text against `".."` misses it.
	///
	/// Returns `None` for a path that does not resolve to something below the base.
	fn target_url(&self, url: &Url) -> Option<ReqwestUrl> {
		let mut segments = Vec::new();
		for segment in url.as_vec() {
			let segment = percent_decode_str(&segment).decode_utf8().ok()?;
			if segment.is_empty() || segment == "." || segment == ".." || segment.contains('/') {
				return None;
			}
			segments.push(segment.into_owned());
		}

		// Nothing to append: the request is the base itself. Handled separately
		// because `pop_if_empty` would drop the base's trailing empty segment and
		// leave `…/assets`, which is a different resource than `…/assets/`.
		if segments.is_empty() {
			return Some(self.base_url.clone());
		}

		let mut target_url = self.base_url.clone();
		target_url
			.path_segments_mut()
			.ok()?
			// The base ends in `/`, i.e. a trailing empty segment. Dropping it
			// first keeps `base/` + `a` from becoming `base//a`.
			.pop_if_empty()
			.extend(segments);

		// The construction above should make this unreachable. It is here because
		// the property that matters — the target stays under the configured base —
		// is worth asserting where a reader can see it, rather than inferring it
		// from the behaviour of a URL library.
		if !target_url.as_str().starts_with(self.base_url.as_str()) {
			return None;
		}

		Some(target_url)
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
		let target_url = self.target_url(url)?;
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

	fn folder() -> RemoteFolder {
		RemoteFolder::from(&ReqwestUrl::parse("https://example.com/assets/").unwrap())
	}

	/// A path that tries to climb above the configured prefix is refused rather
	/// than fetched.
	#[tokio::test]
	async fn dot_segments_are_refused() {
		let folder = folder();

		for request in ["/../secret", "/a/../../secret", "/./../secret"] {
			// Constructed directly: `Url::new` would already have removed these.
			let requested = Url {
				str: request.to_string(),
			};
			assert!(
				folder
					.get_data(&requested, &TargetCompression::from_none())
					.await
					.is_none(),
				"{request} was not refused"
			);
		}
	}

	/// A segment carrying a scheme stays a path segment. `Url::join` would read
	/// it as an absolute URL and fetch a host of the requester's choosing —
	/// cloud metadata, a localhost admin port, anything routable from the server.
	#[test]
	fn a_scheme_in_a_segment_cannot_change_the_host() {
		let folder = folder();

		for request in [
			"/http:/169.254.169.254/latest/meta-data/",
			"/http:/127.0.0.1:9200/_cat/indices",
			"/https:/evil.example/x",
			"/http:/evil.example/x",
			"/file:/etc/passwd",
		] {
			let target = folder.target_url(&Url::from(request)).expect("should resolve");
			assert_eq!(target.host_str(), Some("example.com"), "{request} changed the host");
			assert_eq!(target.scheme(), "https", "{request} changed the scheme");
			assert!(
				target.as_str().starts_with("https://example.com/assets/"),
				"{request} escaped the base: {target}"
			);
		}
	}

	/// `Url::join` treats `%2e%2e` as a dot segment, so a guard comparing the raw
	/// text against `".."` never sees it. Decoding before the check is what makes
	/// these equivalent to their unencoded forms.
	#[test]
	fn percent_encoded_dot_segments_are_refused() {
		let folder = folder();

		for request in [
			"/%2e%2e/secret.txt",
			"/.%2e/secret.txt",
			"/%2e%2E/secret.txt",
			"/a/%2e%2e/%2e%2e/secret.txt",
			"/%2e/secret.txt",
		] {
			assert!(
				folder.target_url(&Url::from(request)).is_none(),
				"{request} was not refused"
			);
		}
	}

	/// A segment that decodes to something containing a separator would smuggle
	/// extra path structure past the per-segment checks.
	#[test]
	fn an_encoded_separator_is_refused() {
		let folder = folder();
		assert!(folder.target_url(&Url::from("/a%2fb/c")).is_none());
	}

	/// The ordinary case still resolves, and percent-encoded names survive the
	/// decode/re-encode round trip unchanged.
	#[test]
	fn ordinary_paths_resolve_below_the_base() {
		let folder = folder();

		for (request, expected) in [
			("/index.html", "https://example.com/assets/index.html"),
			("/sub/dir/style.json", "https://example.com/assets/sub/dir/style.json"),
			("/a%20b.txt", "https://example.com/assets/a%20b.txt"),
			("/caf%C3%A9.json", "https://example.com/assets/caf%C3%A9.json"),
			("/", "https://example.com/assets/"),
		] {
			let target = folder.target_url(&Url::from(request)).expect("should resolve");
			assert_eq!(target.as_str(), expected, "{request}");
		}
	}

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
