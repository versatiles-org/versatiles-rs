use anyhow::{Result, anyhow};
use reqwest::Url;
use std::{
	fmt::Debug,
	path::{Path, PathBuf},
};
use versatiles_derive::context;

#[derive(Clone, PartialEq)]
pub enum UrlPath {
	// An absolute URL with scheme.
	Url(Url),
	// An absolute file path or an relative path/url.
	Path(PathBuf),
}

impl UrlPath {
	pub fn to_string(&self) -> String {
		match self {
			UrlPath::Url(url) => url.to_string(),
			UrlPath::Path(path) => path.to_string_lossy().to_string(),
		}
	}

	#[context("Getting Path from UrlPath {self:?}")]
	pub fn as_path(&self) -> Result<&Path> {
		match self {
			UrlPath::Path(path) => Ok(path.as_path()),
			UrlPath::Url(_) => Err(anyhow!("{self:?} is not a Path")),
		}
	}

	#[context("Resolving UrlPath {self:?} against base {base:?}")]
	pub fn resolve(&mut self, base: &UrlPath) -> Result<()> {
		use UrlPath as UP;
		match (base, &mut *self) {
			// Already an absolute URL -> no-op
			(_, UP::Url(_)) => {}
			// Resolve a Path (relative) against a URL base -> turn into absolute URL
			(UP::Url(base_url), UP::Path(p)) => {
				let s = p.to_str().ok_or_else(|| anyhow!("Invalid Path (non-utf8)"))?;
				*self = UP::Url(base_url.join(s)?);
			}
			// Resolve a Path against a Path base -> join + normalize
			(UP::Path(base_p), UP::Path(p)) => {
				*p = normalize(&base_p.join(&*p));
			}
		}
		Ok(())
	}

	#[context("Getting filename from Url/Path {self:?}")]
	pub fn filename(&self) -> Result<String> {
		let filename = match self {
			UrlPath::Url(url) => url
				.path_segments()
				.ok_or(anyhow!("Invalid URL"))?
				.next_back()
				.ok_or(anyhow!("Invalid URL"))?,
			UrlPath::Path(path) => path
				.file_name()
				.ok_or(anyhow!("Invalid Path"))?
				.to_str()
				.ok_or(anyhow!("Invalid Path"))?,
		};
		Ok(filename.to_string())
	}

	#[context("Getting name without extension from Url/Path {self:?}")]
	pub fn name(&self) -> Result<String> {
		let filename = self.filename()?;
		if let Some(pos) = filename.rfind('.') {
			Ok(filename[..pos].to_string())
		} else {
			Ok(filename)
		}
	}

	#[context("Getting extension from Url/Path {self:?}")]
	pub fn extension(&self) -> Result<String> {
		let filename = self.filename()?;
		if let Some(pos) = filename.rfind('.') {
			Ok(filename[(pos + 1)..].to_string())
		} else {
			Ok("".into())
		}
	}
}

fn normalize(path: &Path) -> PathBuf {
	path
		.components()
		.fold(vec![], |mut acc, component| {
			match component {
				std::path::Component::ParentDir if !acc.is_empty() => {
					acc.pop();
				}
				std::path::Component::CurDir => {}
				_ => acc.push(component.as_os_str()),
			}
			acc
		})
		.into_iter()
		.collect()
}

impl From<String> for UrlPath {
	fn from(s: String) -> Self {
		UrlPath::from(s.as_str())
	}
}

impl From<&str> for UrlPath {
	fn from(s: &str) -> Self {
		if let Ok(url) = reqwest::Url::parse(s)
			&& url.has_host()
		{
			UrlPath::Url(url)
		} else {
			UrlPath::Path(PathBuf::from(s))
		}
	}
}

impl From<&PathBuf> for UrlPath {
	fn from(p: &PathBuf) -> Self {
		UrlPath::Path(p.clone())
	}
}

impl From<PathBuf> for UrlPath {
	fn from(p: PathBuf) -> Self {
		UrlPath::Path(p)
	}
}

impl From<&Path> for UrlPath {
	fn from(p: &Path) -> Self {
		UrlPath::Path(p.to_path_buf())
	}
}

impl From<Url> for UrlPath {
	fn from(u: Url) -> Self {
		UrlPath::Url(u)
	}
}

impl From<&UrlPath> for UrlPath {
	fn from(u: &UrlPath) -> Self {
		u.clone()
	}
}

impl Debug for UrlPath {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			UrlPath::Url(url) => write!(f, "Url({})", url),
			UrlPath::Path(path) => write!(f, "Path({})", path.display()),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	#[rstest]
	#[case("https://example.org/a/b/c.txt", false)]
	#[case("/tmp/hello/world.txt", true)]
	#[case("/tmp/file.txt", true)]
	#[case("https://example.org/file.txt", false)]
	fn as_str_returns_expected_for_url_and_path(#[case] input: &str, #[case] is_path: bool) -> Result<()> {
		let v = UrlPath::from(input);
		assert_eq!(v.to_string(), input);
		assert_eq!(v.as_path().is_ok(), is_path);
		Ok(())
	}

	#[test]
	fn filename_from_url_and_path() -> Result<()> {
		let url = UrlPath::from("https://example.org/assets/data/file.tar.gz");
		let path = UrlPath::from(PathBuf::from("/data/file.txt"));

		assert_eq!(url.filename()?, "file.tar.gz");
		assert_eq!(path.filename()?, "file.txt");
		Ok(())
	}

	#[rstest]
	#[case("../a/b", "../x/y.z", "../a/x/y.z")]
	#[case("../a/b", "./x/y.z", "../a/b/x/y.z")]
	#[case("../a/b", "/x/y.z", "/x/y.z")]
	#[case("../a/b", "x/y.z", "../a/b/x/y.z")]
	#[case("./a/b", "../x/y.z", "a/x/y.z")]
	#[case("./a/b", "./x/y.z", "a/b/x/y.z")]
	#[case("./a/b", "/x/y.z", "/x/y.z")]
	#[case("./a/b", "x/y.z", "a/b/x/y.z")]
	#[case("/a", "ftp://b.org/y.z", "ftp://b.org/y.z")]
	#[case("/a/b", "../x/y.z", "/a/x/y.z")]
	#[case("/a/b", "./x/y.z", "/a/b/x/y.z")]
	#[case("/a/b", "/x/y.z", "/x/y.z")]
	#[case("/a/b", "folder/y.z", "/a/b/folder/y.z")]
	#[case("/a/b", "x/y.z", "/a/b/x/y.z")]
	#[case("a/b", "../x/y.z", "a/x/y.z")]
	#[case("a/b", "./x/y.z", "a/b/x/y.z")]
	#[case("a/b", "/x/y.z", "/x/y.z")]
	#[case("a/b", "x/y.z", "a/b/x/y.z")]
	#[case("ftp://a.org/b/", "../x/y.z", "ftp://a.org/x/y.z")]
	#[case("ftp://a.org/b/", "./x/y.z", "ftp://a.org/b/x/y.z")]
	#[case("ftp://a.org/b/", "/x/y.z", "ftp://a.org/x/y.z")]
	#[case("ftp://a.org/b/", "ftp://b.org/y.z", "ftp://b.org/y.z")]
	#[case("ftp://a.org/b/", "x/y.z", "ftp://a.org/b/x/y.z")]
	fn resolve_matrix(#[case] base: &str, #[case] target: &str, #[case] expected: &str) -> Result<()> {
		let base_up = UrlPath::from(base);
		let mut tgt = UrlPath::from(target);
		tgt.resolve(&base_up)?;
		assert_eq!(tgt.to_string(), expected);
		assert_eq!(tgt, UrlPath::from(expected));
		Ok(())
	}

	#[rstest]
	#[case("https://a.org/b/file.tar.gz", ".gz", "file.tar")]
	#[case("/data/README", "", "README")]
	#[case("/data/README.md", ".md", "README")]
	#[case("/data/README.MD", ".MD", "README")]
	#[case("/data/archive.", ".", "archive")]
	#[case("/data/.bashrc", ".bashrc", "")]
	#[case("https://a.org/dir.with.dots/file", "", "file")]
	fn extension_and_name_matrix(
		#[case] input: &str,
		#[case] expected_ext: &str,
		#[case] expected_name: &str,
	) -> Result<()> {
		let v = UrlPath::from(input);
		assert_eq!(v.extension()?, expected_ext);
		assert_eq!(v.name()?, expected_name);
		Ok(())
	}

	#[rstest]
	#[case("../a/b", "../a/b")]
	#[case("./a/./b", "a/b")]
	#[case("/..", "")]
	#[case("///a//b", "/a/b")]
	#[case("/a/../x", "/x")]
	#[case("/a/./b/.", "/a/b")]
	#[case("a/../../b", "../b")]
	#[case("a/b/../c", "a/c")]
	fn normalize_matrix(#[case] input: &str, #[case] expected: &str) {
		let got = super::normalize(Path::new(input));
		assert_eq!(got, PathBuf::from(expected));
	}

	#[test]
	fn from_conversions_work() -> Result<()> {
		let u = Url::parse("https://example.org/hello.txt")?;
		let up: UrlPath = u.into();
		assert_eq!(up.to_string(), "https://example.org/hello.txt");

		let s = String::from("/tmp/abc.txt");
		let sp: UrlPath = s.into();
		assert_eq!(sp.as_path()?.to_path_buf(), PathBuf::from("/tmp/abc.txt"));

		let sr: UrlPath = "/tmp/xyz.txt".into();
		assert_eq!(sr.as_path()?.to_path_buf(), PathBuf::from("/tmp/xyz.txt"));

		let surl: UrlPath = "https://example.org/a/b".into();
		assert_eq!(surl.to_string(), "https://example.org/a/b");
		Ok(())
	}

	#[test]
	fn debug_impl_is_stable_format_prefix() -> Result<()> {
		let url = UrlPath::from("https://example.org/a/b.txt");
		let path = UrlPath::from(PathBuf::from("/data/c.txt"));

		let d_url = format!("{:?}", url);
		let d_path = format!("{:?}", path);

		assert!(d_url.starts_with("Url(") && d_url.contains("https://example.org/a/b.txt"));
		assert!(d_path.starts_with("Path(") && d_path.contains("c.txt"));
		Ok(())
	}
}
