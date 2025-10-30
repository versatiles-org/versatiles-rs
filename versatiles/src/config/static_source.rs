use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StaticSource {
	pub path: PathBuf,
	pub url_prefix: Option<String>,
}

impl StaticSource {
	pub fn resolve_paths(&mut self, base_path: &Path) {
		if !self.path.is_absolute() {
			self.path = base_path.join(&self.path);
		}
	}
}
