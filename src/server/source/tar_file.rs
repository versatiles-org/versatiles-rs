use crate::{
	create_error,
	server::{guess_mime, make_result, ServerSourceResult, ServerSourceTrait},
	shared::{Blob, Compression, Result, TargetCompression},
};
use async_trait::async_trait;
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
			return create_error!("path {filename:?} does not exist");
		}
		if !filename.is_absolute() {
			return create_error!("path {filename:?} must be absolute");
		}
		if !filename.is_file() {
			return create_error!("path {filename:?} must be a file");
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

	async fn get_data(&mut self, path: &[&str], accept: &TargetCompression) -> Option<ServerSourceResult> {
		let entry_name = path.join("/");
		let file_entry = self.lookup.get(&entry_name)?.to_owned();

		if accept.contains(Compression::Brotli) {
			if let Some(blob) = &file_entry.br {
				return make_result(blob.to_owned(), &Compression::Brotli, &file_entry.mime);
			}
		}

		if accept.contains(Compression::Gzip) {
			if let Some(blob) = &file_entry.gz {
				return make_result(blob.to_owned(), &Compression::Gzip, &file_entry.mime);
			}
		}

		if let Some(blob) = &file_entry.un {
			return make_result(blob.to_owned(), &Compression::None, &file_entry.mime);
		}

		if let Some(blob) = &file_entry.br {
			return make_result(blob.to_owned(), &Compression::Brotli, &file_entry.mime);
		}

		if let Some(blob) = &file_entry.gz {
			return make_result(blob.to_owned(), &Compression::Gzip, &file_entry.mime);
		}

		None
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
	use crate::shared::{TileBBoxPyramid, TileConverterConfig, TileFormat};
	use assert_fs::NamedTempFile;

	pub async fn make_test_tar(compression: Compression) -> NamedTempFile {
		let reader_profile = ReaderProfile::PBF;

		// get dummy reader
		let mut reader = TileReader::new_dummy(reader_profile, 3);

		// get to test container converter
		let container_file = NamedTempFile::new("temp.tar").unwrap();

		let config = TileConverterConfig::new(
			Some(TileFormat::PBF),
			Some(compression),
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
	async fn small_stuff() {
		let file = make_test_tar(Compression::None).await;

		let tar_file = TarFile::from(file.to_str().unwrap()).unwrap();

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

	#[tokio::test]
	async fn test_get_data() {
		use Compression::{Brotli as B, Gzip as G, None as N};

		test1(N).await.unwrap();
		test1(G).await.unwrap();
		test1(B).await.unwrap();

		async fn test1(compression_tar: Compression) -> Result<()> {
			let file = make_test_tar(compression_tar).await;
			let mut tar_file = TarFile::from(file.to_str().unwrap())?;

			test2(&mut tar_file, &compression_tar, N).await?;
			test2(&mut tar_file, &compression_tar, G).await?;
			test2(&mut tar_file, &compression_tar, B).await?;

			return Ok(());

			async fn test2(
				tar_file: &mut TarFile, compression_tar: &Compression, compression_accept: Compression,
			) -> Result<()> {
				let accept = TargetCompression::from(compression_accept);

				let path = ["non_existing_file"];
				let result = tar_file.get_data(&path, &accept).await;
				assert!(result.is_none());

				//let path = ["0", "0", "0"];
				let path = ["tiles.json"];
				let result = tar_file.get_data(&path, &accept).await;
				assert!(result.is_some());

				let result = result.unwrap();

				if result.compression == N {
					assert_eq!(result.blob.as_str(), "dummy meta data");
				}

				assert_eq!(result.mime, "application/json");
				assert_eq!(&result.compression, compression_tar);

				Ok(())
			}
		}
	}
}
