//! This module provides a simple `Url` struct to represent and manipulate internal URL-like paths.
//! It is mainly used within the server for file and directory handling.

use std::path::{Component, Path, PathBuf};

use anyhow::{Result, ensure};
use versatiles_derive::context;

/// Represents a URL-like path starting with `/`.
///
/// This struct is used to handle internal URL paths, ensuring they always start with a `/`.
/// It provides utility methods for common URL operations such as checking prefixes, converting
/// to directory paths, stripping prefixes, and converting to file system paths.
///
/// # Examples
///
/// ```
/// use versatiles::server::Url;
/// let mut url = Url::from("test");
/// assert_eq!(url.to_string(), "/test");
///
/// url = url.to_dir();
/// assert!(url.is_dir());
///
/// let joined = url.join_as_string("file.txt");
/// assert_eq!(joined, "/test/file.txt");
/// ```
#[derive(Clone, PartialOrd, PartialEq, Debug)]
pub struct Url {
	/// The inner string representation of the URL.
	pub str: String,
}

impl Url {
	/// Creates a new `Url` from a string, ensuring it starts with `/` and
	/// contains no `.` or `..` segments.
	///
	/// Dot segments are resolved the way RFC 3986 §5.2.4 requires, and `..` can
	/// never climb above the root: `/../etc/passwd` becomes `/etc/passwd`, not a
	/// path outside whatever the URL is later resolved against. Empty segments
	/// collapse with them, so `//etc/passwd` cannot survive as an absolute path
	/// either. Both matter because a `Url` is joined onto a served directory —
	/// see [`Url::to_pathbug`], which drops such segments a second time.
	///
	/// A trailing slash is preserved, since [`Url::is_dir`] reads it.
	///
	/// # Arguments
	///
	/// * `url` - A string representing the URL path.
	///
	/// # Returns
	///
	/// A new `Url` instance starting with `/`.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles::server::Url;
	/// assert_eq!(Url::from("/a/../b").to_string(), "/b");
	/// assert_eq!(Url::from("/../../etc/passwd").to_string(), "/etc/passwd");
	/// assert_eq!(Url::from("/a/./b/").to_string(), "/a/b/");
	/// ```
	#[must_use]
	pub fn new(url: String) -> Url {
		let str = if url.starts_with('/') { url } else { format!("/{url}") };
		Url {
			str: remove_dot_segments(&str),
		}
	}

	/// Checks if the current `Url` starts with the given prefix `Url`.
	///
	/// # Arguments
	///
	/// * `url` - A reference to another `Url` to check as prefix.
	///
	/// # Returns
	///
	/// `true` if the current `Url` starts with the prefix, otherwise `false`.
	#[must_use]
	pub fn starts_with(&self, url: &Url) -> bool {
		self.str.starts_with(&url.str)
	}

	/// Checks if the `Url` represents a directory, i.e., ends with `/`.
	///
	/// # Returns
	///
	/// `true` if the `Url` ends with `/`, otherwise `false`.
	#[must_use]
	pub fn is_dir(&self) -> bool {
		self.str.ends_with('/')
	}

	/// Converts the `Url` to a directory path by ensuring it ends with `/`.
	///
	/// # Returns
	///
	/// A `Url` guaranteed to represent a directory path.
	#[must_use]
	pub fn to_dir(&self) -> Url {
		if self.str.ends_with('/') {
			self.clone()
		} else {
			Url::new(format!("{}/", self.str))
		}
	}

	/// Strips the given prefix from the `Url`.
	///
	/// # Arguments
	///
	/// * `prefix` - A reference to a `Url` prefix to remove.
	///
	/// # Returns
	///
	/// A `Result` containing the `Url` after prefix removal or an error if the prefix does not match.
	#[context("stripping prefix '{prefix}' from url '{self}'")]
	pub fn strip_prefix(&self, prefix: &Url) -> Result<Url> {
		ensure!(self.str.starts_with(&prefix.str), "url does not start with prefix");

		Ok(Url::from(&self.str[prefix.str.len()..]))
	}

	/// Splits the `Url` path into a vector of non-empty components.
	///
	/// # Returns
	///
	/// A `Vec<String>` containing the path components.
	#[must_use]
	pub fn as_vec(&self) -> Vec<String> {
		self
			.str
			.split('/')
			.filter_map(|s| if s.is_empty() { None } else { Some(s.to_owned()) })
			.collect()
	}

	/// Converts the `Url` into a `PathBuf` relative to the given base path.
	///
	/// # Arguments
	///
	/// * `base` - A base `Path` to join with the `Url` path.
	///
	/// # Returns
	///
	/// A `PathBuf` representing the combined path.
	#[must_use]
	pub fn to_pathbug(&self, base: &Path) -> PathBuf {
		// Appended segment by segment rather than joined as one string: a joined
		// `..` would be resolved by the operating system on `open`, and a joined
		// absolute path would discard `base` entirely. `Url::new` already strips
		// dot segments; this is the second line of defence, and the one that also
		// covers the platform-specific cases — a `\` separator or a `C:` drive
		// prefix inside a single URL segment, which mean nothing on unix but are
		// path syntax on Windows.
		let mut path = base.to_path_buf();
		for segment in self.str.split('/') {
			// Kept only if the segment is exactly one ordinary path component.
			let mut components = Path::new(segment).components();
			if let (Some(Component::Normal(name)), None) = (components.next(), components.next()) {
				path.push(name);
			}
		}
		path
	}

	/// Pushes a filename onto the `Url`, modifying it in place.
	///
	/// # Arguments
	///
	/// * `filename` - A filename to append to the `Url`.
	pub fn push(&mut self, filename: &str) {
		// Via `new`, so a filename carrying dot segments is normalised here too.
		self.str = Url::new(self.join_as_string(filename)).str;
	}

	/// Joins a filename to the `Url` and returns the resulting string.
	///
	/// # Arguments
	///
	/// * `filename` - A filename to append.
	///
	/// # Returns
	///
	/// A `String` representing the joined path.
	#[must_use]
	pub fn join_as_string(&self, filename: &str) -> String {
		if self.is_dir() {
			format!("{}{}", self.str, filename)
		} else {
			format!("{}/{}", self.str, filename)
		}
	}
}

/// Resolves `.` and `..` segments and drops empty ones, per RFC 3986 §5.2.4.
///
/// A `..` with nothing to pop is discarded rather than escaping the root, which
/// is the property the server depends on. `path` is expected to start with `/`,
/// and the result always does.
fn remove_dot_segments(path: &str) -> String {
	// A path ending in a separator, `.` or `..` denotes a directory, and
	// `is_dir` reads that from the string, so the trailing slash is restored
	// after the segments are rebuilt.
	let is_dir = path.ends_with('/') || path.ends_with("/.") || path.ends_with("/..");

	let mut segments: Vec<&str> = Vec::new();
	for segment in path.split('/') {
		match segment {
			"" | "." => {}
			".." => {
				segments.pop();
			}
			s => segments.push(s),
		}
	}

	let mut result = String::with_capacity(path.len() + 1);
	for segment in segments {
		result.push('/');
		result.push_str(segment);
	}
	if result.is_empty() || (is_dir && !result.ends_with('/')) {
		result.push('/');
	}
	result
}

impl std::fmt::Display for Url {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.str)
	}
}

impl From<&str> for Url {
	fn from(s: &str) -> Self {
		Url::new(s.to_owned())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_url_new() {
		assert_eq!(Url::from("test").str, "/test");
		assert_eq!(Url::from("/test").str, "/test");
	}

	/// `..` must be resolved away, never carried into the path a request is
	/// later resolved against, and never able to climb above the root.
	#[test]
	fn dot_segments_are_resolved() {
		assert_eq!(Url::from("/a/../b").str, "/b");
		assert_eq!(Url::from("/a/./b").str, "/a/b");
		assert_eq!(Url::from("/../secret.txt").str, "/secret.txt");
		assert_eq!(Url::from("/../../../../etc/passwd").str, "/etc/passwd");
		assert_eq!(Url::from("/a/b/../..").str, "/");
		assert_eq!(Url::from("//etc/passwd").str, "/etc/passwd");
		assert_eq!(Url::from("/").str, "/");
		assert_eq!(Url::from("").str, "/");
	}

	/// Normalisation must not eat the trailing slash `is_dir` reads.
	#[test]
	fn dot_segment_removal_keeps_directory_marker() {
		assert!(Url::from("/a/./b/").is_dir());
		assert_eq!(Url::from("/a/./b/").str, "/a/b/");
		assert_eq!(Url::from("/a/b/..").str, "/a/");
		assert!(!Url::from("/a/b").is_dir());
	}

	/// `push` normalises too — it is how a directory request becomes
	/// `…/index.html`.
	#[test]
	fn push_normalises() {
		let mut url = Url::from("/assets/");
		url.push("../../etc/passwd");
		assert_eq!(url.str, "/etc/passwd");
	}

	/// The path built for a static file must stay inside the served directory,
	/// whatever the request looked like.
	#[test]
	fn to_pathbug_cannot_escape_the_base() {
		let base = Path::new("/srv/www");
		for request in [
			"/../secret.txt",
			"/../../../../etc/passwd",
			"//etc/passwd",
			"/a/../../secret.txt",
			"/./../secret.txt",
		] {
			let path = Url::from(request).to_pathbug(base);
			assert!(path.starts_with(base), "{request} escaped the base directory: {path:?}");
			assert!(
				!path.components().any(|c| c == Component::ParentDir),
				"{request} left a parent-dir component in {path:?}"
			);
		}
	}

	/// A single segment that is path syntax on some platform must not become one
	/// — `to_pathbug` drops it rather than letting the OS interpret it.
	#[test]
	fn to_pathbug_drops_non_plain_segments() {
		let base = Path::new("/srv/www");
		assert_eq!(Url::from("/a/b").to_pathbug(base), Path::new("/srv/www/a/b"));

		// Constructed directly: `new` would already have removed these.
		let url = Url { str: "/..".to_string() };
		assert_eq!(url.to_pathbug(base), base);
		let url = Url {
			str: "/x/./y".to_string(),
		};
		assert_eq!(url.to_pathbug(base), Path::new("/srv/www/x/y"));
	}

	#[test]
	fn test_starts_with() {
		let base_url = Url::from("/test");

		assert!(Url::from("/test/123").starts_with(&base_url));
		assert!(!Url::from("/123").starts_with(&base_url));
	}

	#[test]
	fn test_is_dir() {
		assert!(Url::from("/test/").is_dir());
		assert!(!Url::from("/test/file.txt").is_dir());
	}

	#[test]
	fn test_strip_prefix() -> Result<()> {
		let full_url = Url::from("/test/dir/file");
		assert_eq!(full_url.strip_prefix(&Url::from("/test"))?.str, "/dir/file");
		assert!(full_url.strip_prefix(&Url::from("/wrong")).is_err());
		Ok(())
	}

	#[test]
	fn test_as_vec() {
		assert_eq!(Url::from("/test/dir/file").as_vec(), vec!["test", "dir", "file"]);
	}

	#[test]
	fn test_as_string() {
		assert_eq!(Url::from("/test/dir/file").to_string(), "/test/dir/file");
	}

	#[test]
	fn test_push() {
		let mut url = Url::from("/test/dir/");
		url.push("file");
		assert_eq!(url.str, "/test/dir/file");

		let mut url = Url::from("/test/dir");
		url.push("file");
		assert_eq!(url.str, "/test/dir/file");
	}

	#[test]
	fn test_be_dir() {
		let mut url = Url::from("/test/dir");
		assert!(!url.is_dir());
		url = url.to_dir();
		assert!(url.is_dir());
		assert_eq!(url.str, "/test/dir/");

		let mut url = Url::from("/test/dir/");
		url = url.to_dir(); // should not change
		assert_eq!(url.str, "/test/dir/");
	}

	#[test]
	fn test_as_path() {
		let url = Url::from("/test/dir/file");
		let path = url.to_pathbug(Path::new("/base"));
		assert_eq!(path, PathBuf::from("/base/test/dir/file"));
	}

	#[test]
	fn test_join_as_string() {
		assert_eq!(Url::from("/test/dir/").join_as_string("file"), "/test/dir/file");

		assert_eq!(Url::from("/test/dir").join_as_string("file"), "/test/dir/file");
	}
}
