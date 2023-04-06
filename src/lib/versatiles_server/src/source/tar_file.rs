use crate::{guess_mime, ok_data, ok_not_found, ServerSourceTrait};
use async_trait::async_trait;
use axum::{
	body::{Bytes, Full},
	response::Response,
};
use enumset::EnumSet;
use log::trace;
use std::{
	collections::HashMap,
	env::current_dir,
	ffi::OsStr,
	fmt::Debug,
	fs::File,
	io::{BufReader, Read},
	path::Path,
};
use tar::{Archive, EntryType};
use versatiles_shared::{compress_brotli, compress_gzip, decompress_brotli, decompress_gzip, Blob, Precompression};

struct FileEntry {
	mime: String,
	un: Option<Blob>,
	gz: Option<Blob>,
	br: Option<Blob>,
}
impl FileEntry {
	fn new(mime: String) -> Self {
		FileEntry {
			mime,
			un: None,
			gz: None,
			br: None,
		}
	}
}

pub struct TarFile {
	lookup: HashMap<String, FileEntry>,
	name: String,
}
impl TarFile {
	pub fn from(path: &str) -> Box<TarFile> {
		let mut filename = current_dir().unwrap();
		filename.push(Path::new(path));
		filename = filename.canonicalize().unwrap();

		assert!(filename.exists(), "path {filename:?} does not exist");
		assert!(filename.is_absolute(), "path {filename:?} must be absolute");
		assert!(filename.is_file(), "path {filename:?} must be a file");

		let mut lookup: HashMap<String, FileEntry> = HashMap::new();
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

			let mut entry_path = file.path().unwrap().into_owned();

			let precompression: Precompression = if let Some(extension) = entry_path.extension() {
				match extension.to_str() {
					Some("br") => Precompression::Brotli,
					Some("gz") => Precompression::Gzip,
					_ => Precompression::Uncompressed,
				}
			} else {
				Precompression::Uncompressed
			};

			if precompression != Precompression::Uncompressed {
				entry_path = entry_path.with_extension("")
			}

			let mut buffer = Vec::new();
			file.read_to_end(&mut buffer).unwrap();
			let blob = Blob::from(buffer);

			let filename = entry_path.file_name().unwrap();

			let mime = &guess_mime(Path::new(&filename));

			let mut add = |path: &Path, blob: Blob| {
				let mut name: String = path
					.iter()
					.map(|s| s.to_str().unwrap())
					.collect::<Vec<&str>>()
					.join("/");

				while name.starts_with(['.', '/']) {
					name = name[1..].to_string();
				}

				trace!("adding file from tar: {} ({:?})", name, precompression);

				let entry = lookup.entry(name);
				let versions = entry.or_insert_with(|| FileEntry::new(mime.to_string()));
				match precompression {
					Precompression::Uncompressed => versions.un = Some(blob),
					Precompression::Gzip => versions.gz = Some(blob),
					Precompression::Brotli => versions.br = Some(blob),
				}
			};

			if filename == OsStr::new("index.html") {
				add(entry_path.parent().unwrap(), blob.clone());
			}
			add(&entry_path, blob);
		}

		Box::new(TarFile {
			lookup,
			name: path.to_string(),
		})
	}
}

#[async_trait]
impl ServerSourceTrait for TarFile {
	fn get_name(&self) -> String {
		self.name.to_owned()
	}
	fn get_info_as_json(&self) -> String {
		"{\"type\":\"tar\"}".to_owned()
	}

	async fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Response<Full<Bytes>> {
		let entry_name = path.join("/");
		let entry_option = self.lookup.get(&entry_name);
		if entry_option.is_none() {
			return ok_not_found();
		}

		let file_entry = entry_option.unwrap().to_owned();

		if accept.contains(Precompression::Brotli) {
			let respond = |blob| ok_data(blob, &Precompression::Brotli, &file_entry.mime);

			if let Some(blob) = &file_entry.br {
				return respond(blob.to_owned());
			}
			if let Some(blob) = &file_entry.un {
				return respond(compress_brotli(blob.to_owned()).unwrap());
			}
			if let Some(blob) = &file_entry.gz {
				return respond(compress_brotli(decompress_gzip(blob.to_owned()).unwrap()).unwrap());
			}
		}

		if accept.contains(Precompression::Gzip) {
			let respond = |blob| ok_data(blob, &Precompression::Gzip, &file_entry.mime);

			if let Some(blob) = &file_entry.gz {
				return respond(blob.to_owned());
			}
			if let Some(blob) = &file_entry.un {
				return respond(compress_gzip(blob.to_owned()).unwrap());
			}
			if let Some(blob) = &file_entry.br {
				return respond(compress_gzip(decompress_brotli(blob.to_owned()).unwrap()).unwrap());
			}
		}

		let respond = |blob| ok_data(blob, &Precompression::Uncompressed, &file_entry.mime);

		if let Some(blob) = &file_entry.un {
			return respond(blob.to_owned());
		}
		if let Some(blob) = &file_entry.br {
			return respond(decompress_brotli(blob.to_owned()).unwrap());
		}
		if let Some(blob) = &file_entry.gz {
			return respond(decompress_gzip(blob.to_owned()).unwrap());
		}

		ok_not_found()
	}
}

impl Debug for TarFile {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TarFile").field("name", &self.name).finish()
	}
}
