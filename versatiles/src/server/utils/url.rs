use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};
use versatiles_derive::context;

#[derive(Clone, PartialOrd, PartialEq, Debug)]
pub struct Url {
	pub str: String,
}

impl Url {
	pub fn new(url: String) -> Url {
		let str = if url.starts_with('/') { url } else { format!("/{url}") };
		Url { str }
	}

	pub fn starts_with(&self, url: &Url) -> bool {
		self.str.starts_with(&url.str)
	}

	pub fn is_dir(&self) -> bool {
		self.str.ends_with('/')
	}

	pub fn to_dir(&self) -> Url {
		if self.str.ends_with('/') {
			self.clone()
		} else {
			Url::new(format!("{}/", self.str))
		}
	}

	#[context("stripping prefix '{prefix}' from url '{self}'")]
	pub fn strip_prefix(&self, prefix: &Url) -> Result<Url> {
		ensure!(self.str.starts_with(&prefix.str), "url does not start with prefix");

		Ok(Url::from(&self.str[prefix.str.len()..]))
	}

	pub fn as_vec(&self) -> Vec<String> {
		self
			.str
			.split('/')
			.filter_map(|s| if s.is_empty() { None } else { Some(s.to_owned()) })
			.collect()
	}

	pub fn to_pathbug(&self, base: &Path) -> PathBuf {
		base.join(&self.str[1..])
	}

	pub fn push(&mut self, filename: &str) {
		self.str = self.join_as_string(filename)
	}

	pub fn join_as_string(&self, filename: &str) -> String {
		if self.is_dir() {
			format!("{}{}", self.str, filename)
		} else {
			format!("{}/{}", self.str, filename)
		}
	}
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
