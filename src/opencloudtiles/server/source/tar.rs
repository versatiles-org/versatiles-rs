use crate::opencloudtiles::{
	lib::{compress_brotli, compress_gzip, Blob, Precompression},
	server::{guess_mime, ok_data, ok_not_found, ServerSourceTrait},
};
use enumset::EnumSet;
use hyper::{Body, Response, Result};
use std::{
	collections::HashMap,
	env::current_dir,
	ffi::OsStr,
	fs::File,
	io::{BufReader, Read},
	path::Path,
};
use tar::{Archive, EntryType};

pub struct Tar {
	lookup: HashMap<String, Blob>,
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

		let mut lookup = HashMap::new();
		let file = BufReader::new(File::open(filename).unwrap());
		let mut archive = Archive::new(file);
		for file_result in archive.entries().unwrap() {
			if file_result.is_err() {
				continue;
			}

			let mut file = file_result.unwrap();

			if file.header().entry_type() != EntryType::Regular {
				continue;
			}

			let mut buffer = Vec::new();
			file.read_to_end(&mut buffer).unwrap();
			let blob = Blob::from_vec(buffer);

			let entry_path = file.path().unwrap();

			if entry_path.file_name() == Some(OsStr::new("index.html")) {
				lookup.insert(path2name(entry_path.parent().unwrap()), blob.clone());
			}

			lookup.insert(path2name(&entry_path), blob);

			fn path2name(path: &Path) -> String {
				path
					.iter()
					.map(|s| s.to_str().unwrap())
					.collect::<Vec<&str>>()
					.join("/")
			}
		}

		Box::new(Tar {
			lookup,
			name: path.to_string(),
		})
	}
}
impl ServerSourceTrait for Tar {
	fn get_name(&self) -> &str { &self.name }

	fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Result<Response<Body>> {
		let entry_name = path.join("/");
		let entry_option = self.lookup.get(&entry_name);
		if entry_option.is_none() {
			return ok_not_found();
		}

		let blob = entry_option.unwrap().to_owned();

		let mime = guess_mime(Path::new(&entry_name));

		if accept.contains(Precompression::Brotli) {
			return ok_data(compress_brotli(blob), &Precompression::Brotli, &mime);
		}

		if accept.contains(Precompression::Gzip) {
			return ok_data(compress_gzip(blob), &Precompression::Gzip, &mime);
		}

		ok_data(blob, &Precompression::Uncompressed, &mime)
	}
}
