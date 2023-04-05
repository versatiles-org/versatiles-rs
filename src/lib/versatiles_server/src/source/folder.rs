use crate::{guess_mime, ok_data, ok_not_found, ServerSourceTrait};
use async_trait::async_trait;
use axum::{
	body::{Bytes, Full},
	response::Response,
};
use enumset::EnumSet;
use std::{
	env::current_dir,
	fmt::Debug,
	fs::File,
	io::{BufReader, Read},
	path::{Path, PathBuf},
};
use versatiles_shared::{compress_brotli, compress_gzip, Blob, Precompression};

pub struct Folder {
	folder: PathBuf,
	name: String,
}

impl Folder {
	pub fn from(path: &str) -> Box<Folder> {
		let mut folder = current_dir().unwrap();
		folder.push(Path::new(path));

		assert!(folder.exists(), "path {folder:?} does not exist");
		assert!(folder.is_absolute(), "path {folder:?} must be absolute");
		assert!(folder.is_dir(), "path {folder:?} must be a directory");

		folder = folder.canonicalize().unwrap();

		Box::new(Folder {
			folder,
			name: path.to_string(),
		})
	}
}

#[async_trait]
impl ServerSourceTrait for Folder {
	fn get_name(&self) -> String {
		self.name.to_owned()
	}
	fn get_info_as_json(&self) -> String {
		"{\"type\":\"folder\"}".to_owned()
	}

	async fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Response<Full<Bytes>> {
		let mut local_path = self.folder.clone();
		local_path.push(PathBuf::from(path.join("/")));

		if local_path.is_dir() {
			local_path.push("index.html");
		}

		if !local_path.starts_with(&self.folder) {
			return ok_not_found();
		}

		if !local_path.exists() {
			return ok_not_found();
		}

		if !local_path.is_file() {
			return ok_not_found();
		}

		let f = File::open(&local_path).unwrap();
		let mut buffer = Vec::new();
		BufReader::new(f).read_to_end(&mut buffer).unwrap();
		let blob = Blob::from(buffer);

		let mime = guess_mime(&local_path);

		if accept.contains(Precompression::Brotli) {
			return ok_data(compress_brotli(blob), &Precompression::Brotli, &mime);
		}

		if accept.contains(Precompression::Gzip) {
			return ok_data(compress_gzip(blob), &Precompression::Gzip, &mime);
		}

		ok_data(blob, &Precompression::Uncompressed, &mime)
	}
}

impl Debug for Folder {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Folder")
			.field("folder", &self.folder)
			.field("name", &self.name)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use axum::body::HttpBody;
	use enumset::enum_set;
	use futures::executor::block_on;
	use hyper::StatusCode;
	use versatiles_shared::Precompression;

	use super::Folder;
	use crate::ServerSourceTrait;

	#[test]
	fn test() {
		block_on(async {
			let folder = Folder::from("../../../ressources");

			assert_eq!(folder.get_name(), "../../../ressources");

			assert_eq!(folder.get_info_as_json(), "{\"type\":\"folder\"}");

			let mut result = folder
				.get_data(&["recipes", "Queijo.txt"], enum_set!(Precompression::Uncompressed))
				.await;
			assert_eq!(result.status(), StatusCode::NOT_FOUND);
			let result = result.data().await.unwrap().unwrap();
			assert_eq!(format!("{:?}", result), "b\"Not Found\"");

			let mut result = folder
				.get_data(&["berlin.mbtiles"], enum_set!(Precompression::Uncompressed))
				.await;
			assert_eq!(result.status(), StatusCode::OK);
			let result = result.data().await.unwrap().unwrap();
			assert_eq!(result.len(), 26533888);
		})
	}
}
