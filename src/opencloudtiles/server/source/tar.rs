use enumset::EnumSet;
use hyper::{Body, Response, Result};
use std::{
	env::current_dir,
	path::{Path, PathBuf},
};

use crate::opencloudtiles::{lib::Precompression, server::ServerSourceTrait};

pub struct Tar {
	filename: PathBuf,
	name: String,
}
impl Tar {
	pub fn from(path: &str) -> Box<Tar> {
		let mut filename = current_dir().unwrap();
		filename.push(Path::new(path));
		filename = filename.canonicalize().unwrap();

		assert!(filename.exists(), "path {:?} does not exist", filename);
		assert!(
			filename.is_absolute(),
			"path {:?} must be absolute",
			filename
		);
		assert!(filename.is_file(), "path {:?} must be a file", filename);

		return Box::new(Tar {
			filename,
			name: path.to_string(),
		});
	}
}
impl ServerSourceTrait for Tar {
	fn get_name(&self) -> &str {
		return &self.name;
	}

	fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Result<Response<Body>> {
		todo!()
	}
}
