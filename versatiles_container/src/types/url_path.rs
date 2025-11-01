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
	Url(reqwest::Url),
	// A file path.
	Path(PathBuf),
}

impl UrlPath {
	pub fn as_str(&self) -> &str {
		match self {
			UrlPath::Url(url) => url.as_str(),
			UrlPath::Path(path) => path.to_str().unwrap_or(""),
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
		match (base, self.clone()) {
			(_, UP::Url(_)) => {
				// URL is already absolute, nothing to do.
			}
			(UP::Url(base), UP::Path(path)) => {
				*self = UP::Url(base.join(path.to_str().ok_or(anyhow!("Invalid Path"))?)?);
			}
			(UP::Path(base_path), UP::Path(mut path)) => {
				path = normalize(&base_path.join(path));
				*self = UP::Path(path);
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
			Ok(filename[pos..].to_string())
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
