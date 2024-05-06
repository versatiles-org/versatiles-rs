use anyhow::{ensure, Result};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Url {
	pub str: String,
}

impl Url {
	pub fn new(url: &str) -> Url {
		let str = if url.starts_with('/') { url.to_owned() } else { format!("/{url}") };
		Url { str }
	}

	pub fn starts_with(&self, url: &Url) -> bool {
		self.str.starts_with(&url.str)
	}

	pub fn is_dir(&self) -> bool {
		self.str.ends_with('/')
	}

	pub fn be_dir(&mut self) {
		if !self.str.ends_with('/') {
			self.str = format!("{}/", self.str)
		}
	}

	pub fn strip_prefix(&self, prefix: &Url) -> Result<Url> {
		ensure!(self.str.starts_with(&prefix.str), "url does not start with prefix");

		Ok(Url::new(&self.str[prefix.str.len()..]))
	}

	pub fn as_vec(&self) -> Vec<String> {
		self
			.str
			.split('/')
			.filter_map(|s| if s.is_empty() { None } else { Some(s.to_owned()) })
			.collect()
	}

	pub fn as_string(&self) -> String {
		self.str.to_owned()
	}

	pub fn as_path(&self, base: &Path) -> PathBuf {
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_url_new() {
		assert_eq!(Url::new("test").str, "/test");
		assert_eq!(Url::new("/test").str, "/test");
	}

	#[test]
	fn test_starts_with() {
		let base_url = Url::new("/test");

		assert!(Url::new("/test/123").starts_with(&base_url));
		assert!(!Url::new("/123").starts_with(&base_url));
	}

	#[test]
	fn test_is_dir() {
		assert!(Url::new("/test/").is_dir());
		assert!(!Url::new("/test/file.txt").is_dir());
	}

	#[test]
	fn test_strip_prefix() -> Result<()> {
		let full_url = Url::new("/test/dir/file");
		assert_eq!(full_url.strip_prefix(&Url::new("/test"))?.str, "/dir/file");
		assert!(full_url.strip_prefix(&Url::new("/wrong")).is_err());
		Ok(())
	}

	#[test]
	fn test_as_vec() {
		assert_eq!(Url::new("/test/dir/file").as_vec(), vec!["test", "dir", "file"]);
	}

	#[test]
	fn test_as_string() {
		assert_eq!(Url::new("/test/dir/file").as_string(), "/test/dir/file");
	}

	#[test]
	fn test_push() {
		let mut url = Url::new("/test/dir/");
		url.push("file");
		assert_eq!(url.str, "/test/dir/file");

		let mut url = Url::new("/test/dir");
		url.push("file");
		assert_eq!(url.str, "/test/dir/file");
	}
}
