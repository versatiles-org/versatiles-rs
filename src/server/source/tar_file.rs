use crate::{
	server::{guess_mime, ok_data, ok_not_found, ServerSourceTrait},
	shared::{compress_brotli, compress_gzip, decompress_brotli, decompress_gzip, Blob, Compression, Error, Result},
};
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
	pub fn from(path: &str) -> Result<Box<Self>> {
		let filename = current_dir()?.join(Path::new(path)).canonicalize()?;

		if !filename.exists() {
			return Err(Error::new(&format!("path {filename:?} does not exist")));
		}
		if !filename.is_absolute() {
			return Err(Error::new(&format!("path {filename:?} must be absolute")));
		}
		if !filename.is_file() {
			return Err(Error::new(&format!("path {filename:?} must be a file")));
		}

		let mut lookup: HashMap<String, FileEntry> = HashMap::new();
		let file = BufReader::new(File::open(filename)?);
		let mut archive = Archive::new(file);

		for file_result in archive.entries()? {
			let mut file = match file_result {
				Ok(file) => file,
				Err(_) => continue,
			};

			if file.header().entry_type() != EntryType::Regular {
				continue;
			}

			let mut entry_path = file.path()?.into_owned();
			let compression = entry_path
				.extension()
				.and_then(OsStr::to_str)
				.map(|ext| match ext {
					"br" => Compression::Brotli,
					"gz" => Compression::Gzip,
					_ => Compression::None,
				})
				.unwrap_or(Compression::None);

			if compression != Compression::None {
				entry_path.set_extension("");
			}

			let mut buffer = Vec::new();
			file.read_to_end(&mut buffer)?;
			let blob = Blob::from(buffer);

			let filename = entry_path.file_name().unwrap();
			let mime = guess_mime(Path::new(&filename));

			let mut add = |path: &Path, blob: Blob| {
				let mut name = path
					.iter()
					.map(|s| s.to_str().unwrap())
					.collect::<Vec<&str>>()
					.join("/");

				while name.starts_with(['.', '/']) {
					name = name[1..].to_string();
				}

				trace!("Adding file from tar: {} ({:?})", name, compression);

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

		Ok(Box::new(Self {
			lookup,
			name: path.to_string(),
		}))
	}
}

#[async_trait]
impl ServerSourceTrait for TarFile {
	fn get_name(&self) -> Result<String> {
		Ok(self.name.to_owned())
	}
	fn get_info_as_json(&self) -> Result<String> {
		Ok("{\"type\":\"tar\"}".to_owned())
	}

	async fn get_data(&mut self, path: &[&str], accept: EnumSet<Compression>) -> Response<Full<Bytes>> {
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
	use crate::containers::{
		dummy::{ReaderProfile, TileReader},
		tar::TileConverter,
		TileConverterTrait,
	};
	use crate::shared::{decompress, TileBBoxPyramid, TileConverterConfig, TileFormat};
	use assert_fs::NamedTempFile;
	use axum::body::HttpBody;
	use enumset::enum_set;
	use hyper::header::CONTENT_ENCODING;

	async fn get_as_string(container: &mut Box<TarFile>, path: &[&str], compression: &Compression) -> String {
		let mut resp = container.get_data(path, enum_set!(compression)).await;
		let encoding = resp.headers().get(CONTENT_ENCODING);

		let content_compression = match encoding {
			None => Compression::None,
			Some(value) => match value.to_str().unwrap() {
				"gzip" => Compression::Gzip,
				"br" => Compression::Brotli,
				_ => panic!("encoding: {encoding:?}"),
			},
		};

		let data = resp.data().await.unwrap().unwrap();
		let data = decompress(Blob::from(data), &content_compression).unwrap();
		let data = String::from_utf8_lossy(data.as_slice());
		return data.to_string();
	}

	pub async fn make_test_tar(compression: &Compression) -> NamedTempFile {
		let reader_profile = ReaderProfile::PbfFast;

		// get dummy reader
		let mut reader = TileReader::new_dummy(reader_profile, 3);

		// get to test container converter
		let container_file = NamedTempFile::new("temp.tar").unwrap();

		let config = TileConverterConfig::new(
			Some(TileFormat::PBF),
			Some(compression.to_owned()),
			TileBBoxPyramid::new_full(),
			false,
		);
		let mut converter = TileConverter::new(container_file.to_str().unwrap(), config)
			.await
			.unwrap();

		// convert
		converter.convert_from(&mut reader).await.unwrap();

		container_file
	}

	#[tokio::test]
	async fn compressions() {
		use Compression::*;

		async fn test_compression(from_compression: &Compression, to_compression: &Compression) {
			let file = make_test_tar(from_compression).await;

			let mut tar_file = TarFile::from(&file.to_str().unwrap()).unwrap();

			let result = get_as_string(&mut tar_file, &["meta.json"], to_compression).await;
			assert_eq!(result, "Not Found");

			let result = get_as_string(&mut tar_file, &["tiles.json"], to_compression).await;
			assert_eq!(result, "dummy meta data");

			let result = get_as_string(&mut tar_file, &["0", "0", "0.pbf"], to_compression).await;
			assert!(result.starts_with("\u{1a}4\n\u{5}ocean"));

			let result = get_as_string(&mut tar_file, &["cheesecake.mp4"], to_compression).await;
			assert_eq!(result, "Not Found");
		}

		test_compression(&None, &None).await;
		test_compression(&None, &Gzip).await;
		test_compression(&None, &Brotli).await;

		test_compression(&Gzip, &None).await;
		test_compression(&Gzip, &Gzip).await;
		test_compression(&Gzip, &Brotli).await;

		test_compression(&Brotli, &None).await;
		test_compression(&Brotli, &Gzip).await;
		test_compression(&Brotli, &Brotli).await;
	}

	#[tokio::test]
	async fn small_stuff() {
		let file = make_test_tar(&Compression::None).await;

		let tar_file = TarFile::from(&file.to_str().unwrap()).unwrap();

		assert_eq!(tar_file.get_info_as_json().unwrap(), "{\"type\":\"tar\"}");
		assert!(tar_file.get_name().unwrap().ends_with("temp.tar"));
		assert!(format!("{:?}", tar_file).starts_with("TarFile { name:"));
	}

	#[test]
	fn from_non_existing_path() {
		let path = "path/to/non-existing/file.tar";
		assert!(TarFile::from(path).is_err());
	}

	#[test]
	fn from_directory() {
		let path = ".";
		assert!(TarFile::from(path).is_err());
	}
}
