use serde::{Deserialize, Serialize};
use std::{
	fmt::Debug,
	path::{Path, PathBuf},
};

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TileSource {
	pub name: Option<String>,
	pub path: PathBuf,

	#[serde(default)]
	pub flip_y: Option<bool>,

	#[serde(default)]
	pub swap_xy: Option<bool>,

	#[serde(default)]
	pub override_compression: Option<String>,
}

impl TileSource {
	pub fn resolve_paths(&mut self, base_path: &Path) {
		if !self.path.is_absolute() {
			self.path = base_path.join(&self.path);
		}
	}
}
