//! `DataLocation` is a general abstraction representing the location of data,
//! supporting URLs, filesystem paths, and in-memory blobs.
//!
//! It is used to accept flexible inputs in CLI tools and server components and provides
//! convenience methods like `resolve`, `filename`, and `extension`.
//!
//! The enum has three variants:
//! - `Url(reqwest::Url)` for absolute URLs (e.g., `https://example.org/file.txt`)
//! - `Path(std::path::PathBuf)` for absolute or relative filesystem paths (e.g., `/data/a.txt` or `./a/b`)
//! - `Blob(Blob)` for in-memory data blobs

use anyhow::{Result, anyhow, bail};
use reqwest::Url;
use std::{
	fmt::Debug,
	path::{Path, PathBuf},
};
use versatiles_core::Blob;
use versatiles_derive::context;

/// A flexible location of data used across I/O code.
///
/// # Examples
/// Creating from strings:
/// ```
/// use versatiles_container::DataLocation;
/// let a = DataLocation::from("https://example.org/x.png");
/// let b = DataLocation::from("./data/x.png");
/// assert!(matches!(a, DataLocation::Url(_)));
/// assert!(matches!(b, DataLocation::Path(_)));
/// ```
/// Resolving against a base:
/// ```
/// # use versatiles_container::DataLocation;
/// let base = DataLocation::from("/tiles/");
/// let mut tgt = DataLocation::from("z/x/y.mvt");
/// tgt.resolve(&base).unwrap();
/// assert_eq!(tgt.to_string().replace('\\', "/"), "/tiles/z/x/y.mvt");
/// ```
#[derive(Clone, PartialEq)]
pub enum DataLocation {
	/// An absolute URL with scheme.
	Url(Url),
	/// An absolute file path or a relative path/url.
	Path(PathBuf),
	/// In-memory blob data.
	Blob(Blob),
}

impl DataLocation {
	/// Borrow the underlying filesystem path.
	///
	/// Returns an error if this value is a URL or Blob.
	///
	/// Useful when the caller expects a path-only input and wants a clear error otherwise.
	#[context("Getting filesystem path from DataLocation {self:?}")]
	pub fn as_path(&self) -> Result<&Path> {
		match self {
			DataLocation::Path(path) => Ok(path.as_path()),
			_ => Err(anyhow!("{self:?} is not a Path")),
		}
	}

	/// Resolve this value against `base` in-place.
	///
	/// Rules:
	/// - If `self` is already a URL, it remains unchanged.
	/// - If `base` is a URL and `self` is a relative path, `self` becomes a URL via `base.join()`.
	/// - If both are paths, they are joined and normalized (handling `.` and `..`).
	///
	/// Returns an error only when URL joining fails or inputs are invalid.
	#[context("Resolving DataLocation {self:?} against base {base:?}")]
	pub fn resolve(&mut self, base: &DataLocation) -> Result<()> {
		use DataLocation as UP;
		match (base, &mut *self) {
			// Resolve a Path (relative) against a URL base -> turn into absolute URL
			(UP::Url(base_url), UP::Path(p)) => {
				let s = p.to_str().ok_or_else(|| anyhow!("Invalid Path (non-utf8)"))?;
				*self = UP::Url(base_url.join(s)?);
			}
			// Resolve a Path against a Path base -> join + normalize
			(UP::Path(base_p), UP::Path(p)) => {
				*p = normalize(&base_p.join(&*p));
			}
			// All other combinations leave `self` unchanged
			(_, _) => {}
		}
		Ok(())
	}

	/// Return the last path segment (e.g., `file.tar.gz`).
	///
	/// For URLs, the segment is derived from the URL path. For filesystem paths, it is the
	/// filename component. Errors if the source has no terminal segment.
	#[context("Getting filename from DataLocation {self:?}")]
	pub fn filename(&self) -> Result<String> {
		let filename = match self {
			DataLocation::Url(url) => url
				.path_segments()
				.ok_or(anyhow!("Invalid URL"))?
				.next_back()
				.ok_or(anyhow!("Invalid URL"))?,
			DataLocation::Path(path) => path
				.file_name()
				.ok_or(anyhow!("Invalid Path"))?
				.to_str()
				.ok_or(anyhow!("Invalid Path"))?,
			DataLocation::Blob(_) => bail!("Blob has no filename"),
		};
		Ok(filename.to_string())
	}

	/// Return the filename **without** its last extension.
	///
	/// `"/a/file.tar.gz" -> "file.tar"`, `"/a/README" -> "README"`.
	#[context("Getting name without extension from DataLocation {self:?}")]
	pub fn name(&self) -> Result<String> {
		let filename = self.filename()?;
		if let Some(pos) = filename.rfind('.') {
			Ok(filename[..pos].to_string())
		} else {
			Ok(filename)
		}
	}

	/// Return the filename's last extension (without the dot), or an empty string if none.
	///
	/// `"/a/file.tar.gz" -> "gz"`, `"/a/README" -> ""`.
	#[context("Getting extension from DataLocation {self:?}")]
	pub fn extension(&self) -> Result<String> {
		let filename = self.filename()?;
		if let Some(pos) = filename.rfind('.') {
			Ok(filename[(pos + 1)..].to_string())
		} else {
			Ok("".into())
		}
	}

	pub fn cwd() -> Result<Self> {
		Ok(DataLocation::Path(std::env::current_dir()?))
	}
}

// Normalize a path by resolving `.` and `..`, preserving drive prefixes, and handling
// Windows UNC shares (`\\server\\share`). Relative parents (`..`) are preserved if there
// is nothing left to pop. Used by `resolve` for Path+Path cases.
fn normalize(path: &Path) -> PathBuf {
	use std::ffi::OsString;
	use std::path::Component::*;

	let mut prefix: Option<OsString> = None;
	let mut is_abs = false;
	let mut parts: Vec<OsString> = Vec::new();
	let mut leading_parents: usize = 0;

	#[cfg(windows)]
	let mut is_unc: bool = false;
	#[cfg(windows)]
	let mut unc_share_consumed: bool = false;

	for comp in path.components() {
		match comp {
			Prefix(p) => {
				prefix = Some(p.as_os_str().to_os_string());
				#[cfg(windows)]
				{
					use std::path::Prefix::*;
					is_unc = matches!(p.kind(), UNC(_, _) | VerbatimUNC(_, _));
				}
			}
			RootDir => {
				is_abs = true;
			}
			CurDir => {}
			ParentDir => {
				if !parts.is_empty() {
					#[cfg(windows)]
					{
						// Only protect the share for UNC paths.
						if !(is_abs && is_unc && unc_share_consumed && parts.len() == 1) {
							let _ = parts.pop();
						}
					}
					#[cfg(not(windows))]
					{
						let _ = parts.pop();
					}
				} else if !is_abs {
					leading_parents += 1;
				}
			}
			Normal(s) => {
				#[cfg(windows)]
				{
					// The first normal component after \\server\ (with RootDir) is the share.
					if is_abs && is_unc && !unc_share_consumed && parts.is_empty() {
						parts.push(s.to_os_string());
						unc_share_consumed = true;
					} else {
						parts.push(s.to_os_string());
					}
				}
				#[cfg(not(windows))]
				{
					parts.push(s.to_os_string());
				}
			}
		}
	}

	let mut out = PathBuf::new();
	if let Some(p) = prefix {
		out.push(p);
	}
	if is_abs {
		#[cfg(windows)]
		{
			out.push(Path::new("\\"));
		}
		#[cfg(not(windows))]
		{
			out.push(Path::new("/"));
		}
	} else {
		for _ in 0..leading_parents {
			out.push("..");
		}
	}
	for seg in parts {
		out.push(seg);
	}
	out
}

/// Display as a plain URL string for `Url`, or as a path using `Path::display()` for `Path`.
impl std::fmt::Display for DataLocation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			DataLocation::Url(url) => write!(f, "{url}"),
			DataLocation::Path(path) => write!(f, "{}", path.display()),
			DataLocation::Blob(blob) => write!(f, "<blob len={}>", blob.len()),
		}
	}
}

impl From<String> for DataLocation {
	fn from(s: String) -> Self {
		DataLocation::from(s.as_str())
	}
}

impl From<&str> for DataLocation {
	fn from(s: &str) -> Self {
		if let Ok(url) = reqwest::Url::parse(s)
			&& url.has_host()
		{
			DataLocation::Url(url)
		} else {
			DataLocation::Path(PathBuf::from(s))
		}
	}
}

/// Construct from `&str` by attempting URL parsing first. If parsing succeeds and the value
/// has a host, the `Url` variant is used; otherwise the value is treated as a filesystem path.
impl std::str::FromStr for DataLocation {
	type Err = anyhow::Error;
	fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
		Ok(DataLocation::from(s))
	}
}

impl From<&PathBuf> for DataLocation {
	fn from(p: &PathBuf) -> Self {
		DataLocation::Path(p.clone())
	}
}

impl From<PathBuf> for DataLocation {
	fn from(p: PathBuf) -> Self {
		DataLocation::Path(p)
	}
}

impl From<&Path> for DataLocation {
	fn from(p: &Path) -> Self {
		DataLocation::Path(p.to_path_buf())
	}
}

impl From<Url> for DataLocation {
	fn from(u: Url) -> Self {
		DataLocation::Url(u)
	}
}

impl From<&DataLocation> for DataLocation {
	fn from(u: &DataLocation) -> Self {
		u.clone()
	}
}

impl Debug for DataLocation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			DataLocation::Url(url) => write!(f, "Url({})", url),
			DataLocation::Path(path) => write!(f, "Path({})", path.display()),
			DataLocation::Blob(blob) => write!(f, "Blob(len={})", blob.len()),
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
		let v = DataLocation::from(input);
		assert_eq!(v.as_path().is_ok(), is_path);

		match &v {
			DataLocation::Url(u) => {
				assert!(!is_path, "expected a URL case");
				assert_eq!(u.as_str(), input);
			}
			DataLocation::Path(p) => {
				assert!(is_path, "expected a path case");
				assert_eq!(p.to_path_buf(), PathBuf::from(input));
			}
			DataLocation::Blob(_) => {
				bail!("Unexpected Blob variant");
			}
		}
		Ok(())
	}

	#[test]
	fn filename_from_url_and_path() -> Result<()> {
		let url = DataLocation::from("https://example.org/assets/data/file.tar.gz");
		let path = DataLocation::from(PathBuf::from("/data/file.txt"));

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
		let base_up = DataLocation::from(base);
		let mut tgt = DataLocation::from(target);
		tgt.resolve(&base_up)?;
		// Compare URLs as strings, but file paths as normalized paths (to allow \\ on Windows).
		match &tgt {
			DataLocation::Url(u) => {
				assert_eq!(u.as_str(), expected);
			}
			DataLocation::Path(p) => {
				let got = p.to_path_buf();
				let want = PathBuf::from(expected);
				assert_eq!(got, want);
			}
			DataLocation::Blob(_) => bail!("Unexpected Blob variant"),
		}
		Ok(())
	}

	#[rstest]
	#[case("https://a.org/b/file.tar.gz", "gz", "file.tar")]
	#[case("/data/README", "", "README")]
	#[case("/data/README.md", "md", "README")]
	#[case("/data/README.MD", "MD", "README")]
	#[case("/data/archive.", "", "archive")]
	#[case("/data/.bashrc", "bashrc", "")]
	#[case("https://a.org/dir.with.dots/file", "", "file")]
	fn extension_and_name_matrix(
		#[case] input: &str,
		#[case] expected_ext: &str,
		#[case] expected_name: &str,
	) -> Result<()> {
		let v = DataLocation::from(input);
		assert_eq!(v.extension()?, expected_ext);
		assert_eq!(v.name()?, expected_name);
		Ok(())
	}

	#[rstest]
	#[case("../a/b", "../a/b")]
	#[case("./a/./b", "a/b")]
	#[case("/..", "/")]
	#[case("///a//b", "/a/b")]
	#[case("/a/../x", "/x")]
	#[case("/a/./b/.", "/a/b")]
	#[case("a/../../b", "../b")]
	#[case("a/b/../c", "a/c")]
	fn normalize_matrix(#[case] input: &str, #[case] expected: &str) {
		let got = normalize(Path::new(input)).display().to_string().replace('\\', "/");
		assert_eq!(&got, expected);
	}

	#[cfg(windows)]
	#[rstest]
	#[case(r"C:\a\..\b", r"C:\b")]
	#[case(r"C:\a\.\b\.", r"C:\a\b")]
	#[case(r"C:\..\..", r"C:\")]
	#[case(r"\\server\share\..\x", r"\\server\share\x")]
	fn normalize_windows_matrix(#[case] input: &str, #[case] expected: &str) {
		assert_eq!(super::normalize(Path::new(input)), PathBuf::from(expected));
	}

	#[test]
	fn from_conversions_work() -> Result<()> {
		let u = Url::parse("https://example.org/hello.txt")?;
		let up: DataLocation = u.into();
		assert_eq!(up.to_string(), "https://example.org/hello.txt");

		let s = String::from("/tmp/abc.txt");
		let sp: DataLocation = s.into();
		assert_eq!(sp.as_path()?.to_path_buf(), PathBuf::from("/tmp/abc.txt"));

		let sr: DataLocation = "/tmp/xyz.txt".into();
		assert_eq!(sr.as_path()?.to_path_buf(), PathBuf::from("/tmp/xyz.txt"));

		let surl: DataLocation = "https://example.org/a/b".into();
		assert_eq!(surl.to_string(), "https://example.org/a/b");
		Ok(())
	}

	#[test]
	fn debug_impl_is_stable_format_prefix() -> Result<()> {
		let url = DataLocation::from("https://example.org/a/b.txt");
		let path = DataLocation::from(PathBuf::from("/data/c.txt"));

		let d_url = format!("{:?}", url);
		let d_path = format!("{:?}", path);

		assert!(d_url.starts_with("Url(") && d_url.contains("https://example.org/a/b.txt"));
		assert!(d_path.starts_with("Path(") && d_path.contains("c.txt"));
		Ok(())
	}
}
