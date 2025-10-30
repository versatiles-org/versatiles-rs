use anyhow::{Result, anyhow};
use reqwest::Url;
use std::{
	fmt::Debug,
	path::{Path, PathBuf},
};
use versatiles_derive::context;

#[derive(Clone, PartialEq)]
pub enum UrlPath {
	Url(reqwest::Url),
	Path(PathBuf),
}

impl UrlPath {
	pub fn as_str(&self) -> &str {
		match self {
			UrlPath::Url(url) => url.as_str(),
			UrlPath::Path(path) => path.to_str().unwrap_or(""),
		}
	}

	pub fn as_path(&self) -> Result<&Path> {
		match self {
			UrlPath::Path(path) => Ok(path.as_path()),
			UrlPath::Url(_) => Err(anyhow!("{self:?} is not a Path")),
		}
	}

	pub fn resolve(&mut self, base: &UrlPath) -> Result<()> {
		use UrlPath as U;
		match (self.clone(), base) {
			(U::Url(url), U::Url(base_url)) => {
				if url.has_host() {
					return Ok(());
				}
				*self = U::Url(base_url.clone().join(url.as_str())?);
			}
			(U::Path(path), U::Path(base_path)) => {
				if path.is_absolute() {
					return Ok(());
				}
				*self = U::Path(base_path.join(path));
			}
			_ => (),
		}
		Ok(())
	}

	#[context("Getting filename from Url/Path {:?}", self)]
	pub fn filename(&self) -> Result<String> {
		let filename = match self {
			UrlPath::Url(url) => url
				.path_segments()
				.ok_or(anyhow!("Invalid URL"))?
				.last()
				.ok_or(anyhow!("Invalid URL"))?,
			UrlPath::Path(path) => path
				.file_name()
				.ok_or(anyhow!("Invalid Path"))?
				.to_str()
				.ok_or(anyhow!("Invalid Path"))?,
		};
		Ok(filename.to_string())
	}

	// Get a name, like the filename without extension.
	pub fn name(&self) -> Result<String> {
		let filename = self.filename()?;
		if let Some(pos) = filename.rfind('.') {
			Ok(filename[..pos].to_string())
		} else {
			Ok(filename)
		}
	}

	pub fn extension(&self) -> Result<String> {
		let filename = self.filename()?;
		if let Some(pos) = filename.rfind('.') {
			Ok(filename[pos..].to_string())
		} else {
			Ok("".into())
		}
	}
}

impl From<String> for UrlPath {
	fn from(s: String) -> Self {
		UrlPath::from(s.as_str())
	}
}

impl From<&str> for UrlPath {
	fn from(s: &str) -> Self {
		if let Ok(url) = reqwest::Url::parse(s) {
			UrlPath::Url(url)
		} else {
			UrlPath::Path(PathBuf::from(s))
		}
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

impl Debug for UrlPath {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			UrlPath::Url(url) => write!(f, "Url({})", url),
			UrlPath::Path(path) => write!(f, "Path({})", path.display()),
		}
	}
}
