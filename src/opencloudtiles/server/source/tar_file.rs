use crate::opencloudtiles::{
	lib::*,
	server::{guess_mime, ok_data, ok_not_found, ServerSourceTrait},
};
use enumset::EnumSet;
use hyper::{Body, Response, Result};
use log::trace;
use std::{
	collections::HashMap,
	env::current_dir,
	ffi::OsStr,
	fs::File,
	io::{BufReader, Read},
	path::Path,
};
use tar::{Archive, EntryType};

struct CompressedVersions {
	un: Option<Blob>,
	gz: Option<Blob>,
	br: Option<Blob>,
}
impl CompressedVersions {
	fn new() -> Self {
		CompressedVersions {
			un: None,
			gz: None,
			br: None,
		}
	}
}

pub struct TarFile {
	lookup: HashMap<String, CompressedVersions>,
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

		let mut lookup: HashMap<String, CompressedVersions> = HashMap::new();
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
			let blob = Blob::from_vec(buffer);

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
				let versions = entry.or_insert_with(CompressedVersions::new);
				match precompression {
					Precompression::Uncompressed => versions.un = Some(blob),
					Precompression::Gzip => versions.gz = Some(blob),
					Precompression::Brotli => versions.br = Some(blob),
				}
			};

			if entry_path.file_name() == Some(OsStr::new("index.html")) {
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
impl ServerSourceTrait for TarFile {
	fn get_name(&self) -> &str {
		&self.name
	}

	fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Result<Response<Body>> {
		let entry_name = path.join("/");
		let entry_option = self.lookup.get(&entry_name);
		if entry_option.is_none() {
			return ok_not_found();
		}

		let versions = entry_option.unwrap().to_owned();

		let mime = guess_mime(Path::new(&entry_name));

		if accept.contains(Precompression::Brotli) {
			let respond = |blob| ok_data(blob, &Precompression::Brotli, &mime);

			if let Some(blob) = &versions.br {
				return respond(blob.to_owned());
			}
			if let Some(blob) = &versions.un {
				return respond(compress_brotli(blob.to_owned()));
			}
			if let Some(blob) = &versions.gz {
				return respond(compress_brotli(decompress_gzip(blob.to_owned())));
			}
		}

		if accept.contains(Precompression::Gzip) {
			let respond = |blob| ok_data(blob, &Precompression::Gzip, &mime);

			if let Some(blob) = &versions.gz {
				return respond(blob.to_owned());
			}
			if let Some(blob) = &versions.un {
				return respond(compress_gzip(blob.to_owned()));
			}
			if let Some(blob) = &versions.br {
				return respond(compress_gzip(decompress_brotli(blob.to_owned())));
			}
		}

		let respond = |blob| ok_data(blob, &Precompression::Uncompressed, &mime);

		if let Some(blob) = &versions.un {
			return respond(blob.to_owned());
		}
		if let Some(blob) = &versions.br {
			return respond(decompress_brotli(blob.to_owned()));
		}
		if let Some(blob) = &versions.gz {
			return respond(decompress_gzip(blob.to_owned()));
		}

		ok_not_found()
	}
}
