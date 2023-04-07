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
use versatiles_shared::{compress_brotli, compress_gzip, decompress_brotli, decompress_gzip, Blob, Compression};

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

			let compression: Compression = if let Some(extension) = entry_path.extension() {
				match extension.to_str() {
					Some("br") => Compression::Brotli,
					Some("gz") => Compression::Gzip,
					_ => Compression::None,
				}
			} else {
				Compression::None
			};

			if compression != Compression::None {
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

				trace!("adding file from tar: {} ({:?})", name, compression);

				let entry = lookup.entry(name);
				let versions = entry.or_insert_with(|| FileEntry::new(mime.to_string()));
				match compression {
					Compression::None => versions.un = Some(blob),
					Compression::Gzip => versions.gz = Some(blob),
					Compression::Brotli => versions.br = Some(blob),
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

	async fn get_data(&self, path: &[&str], accept: EnumSet<Compression>) -> Response<Full<Bytes>> {
		let entry_name = path.join("/");
		let entry_option = self.lookup.get(&entry_name);
		if entry_option.is_none() {
			return ok_not_found();
		}

		let file_entry = entry_option.unwrap().to_owned();

		if accept.contains(Compression::Brotli) {
			let respond = |blob| ok_data(blob, &Compression::Brotli, &file_entry.mime);

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

		if accept.contains(Compression::Gzip) {
			let respond = |blob| ok_data(blob, &Compression::Gzip, &file_entry.mime);

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

		let respond = |blob| ok_data(blob, &Compression::None, &file_entry.mime);

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
#[cfg(test)]
mod tests {
	use super::*;
	use assert_fs::NamedTempFile;
	use axum::body::HttpBody;
	use enumset::enum_set;
	use versatiles_container::{
		dummy::{ReaderProfile, TileReader},
		tar::TileConverter,
		TileConverterTrait,
	};
	use versatiles_shared::{TileBBoxPyramide, TileConverterConfig, TileFormat};

	async fn get_as_string(container: &Box<TarFile>, path: &[&str], compression: Compression) -> String {
		let mut resp = container.get_data(path, enum_set!(compression)).await;
		let data1 = resp.data().await.unwrap().unwrap();
		let data3 = String::from_utf8_lossy(&data1);
		return data3.to_string();
	}

	pub async fn make_test_tar(compression: Compression) -> NamedTempFile {
		let reader_profile = ReaderProfile::PbfFast;

		// get dummy reader
		let mut reader = TileReader::new_dummy(reader_profile, 3);

		// get to test container comverter
		let container_file = NamedTempFile::new("temp.tar").unwrap();

		let config = TileConverterConfig::new(
			Some(TileFormat::PBF),
			Some(compression),
			TileBBoxPyramide::new_full(),
			false,
		);
		let mut converter = TileConverter::new(&container_file.path(), config);

		// convert
		converter.convert_from(&mut reader).await;

		container_file
	}
	async fn test_tar_file(compression: Compression) {
		let file = make_test_tar(compression).await;

		let tar_file = TarFile::from(&file.to_str().unwrap());

		let result = get_as_string(&tar_file, &["meta.json"], Compression::None).await;
		assert_eq!(result, "dummy meta data");

		let result = get_as_string(&tar_file, &["0", "0", "0.pbf"], Compression::None).await;
		println!("{}", result);
		assert!(result.starts_with("\u{1a}4\n\u{5}ocean"));

		let result = get_as_string(&tar_file, &["cheesecake.mp4"], Compression::None).await;
		assert_eq!(result, "Not Found");
	}

	#[tokio::test]
	async fn test_tar_file_uncompressed() {
		test_tar_file(Compression::None).await;
	}

	#[tokio::test]
	async fn test_tar_file_gzip() {
		test_tar_file(Compression::Gzip).await;
	}

	#[tokio::test]
	async fn test_tar_file_brotli() {
		test_tar_file(Compression::Brotli).await;
	}

	#[test]
	fn test_tar_file_from_non_existing_path() {
		let path = "path/to/non-existing/file.tar";
		let result = std::panic::catch_unwind(|| TarFile::from(path));
		assert!(result.is_err());
	}

	#[test]
	fn test_tar_file_from_directory() {
		let path = ".";
		let result = std::panic::catch_unwind(|| TarFile::from(path));
		assert!(result.is_err());
	}
}
