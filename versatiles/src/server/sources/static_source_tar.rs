use super::super::utils::{Url, guess_mime};
use super::{SourceResponse, static_source::StaticSourceTrait};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use std::{collections::HashMap, env::current_dir, ffi::OsStr, fmt::Debug, fs::File, io::Read, path::Path};
use tar::{Archive, EntryType};
use versatiles_core::{
	Blob, TileCompression,
	utils::{TargetCompression, decompress_brotli, decompress_gzip},
};
use versatiles_derive::context;

#[derive(Debug)]
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
	#[context("loading static tar file from path: {path:?}")]
	pub fn from(path: &Path) -> Result<Self> {
		use TileCompression::*;

		let path = current_dir()?.join(path).canonicalize()?;

		ensure!(path.exists(), "path {path:?} does not exist");
		ensure!(path.is_absolute(), "path {path:?} must be absolute");
		ensure!(path.is_file(), "path {path:?} must be a file");

		let mut file = File::open(&path)?;
		let mut buffer: Vec<u8> = Vec::new();
		file.read_to_end(&mut buffer)?;
		let mut buffer = Blob::from(buffer);
		drop(file);

		for part in path.to_str().unwrap().rsplit('.') {
			match part {
				"tar" => break,
				"gz" => buffer = decompress_gzip(&buffer)?,
				"br" => buffer = decompress_brotli(&buffer)?,
				_ => bail!("{path:?} must be a name of a tar file"),
			}
		}

		let mut archive = Archive::new(buffer.as_slice());

		let mut lookup: HashMap<String, FileEntry> = HashMap::new();
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
					"br" => Brotli,
					"gz" => Gzip,
					_ => Uncompressed,
				})
				.unwrap_or(Uncompressed);

			if compression != Uncompressed {
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

				log::trace!("Adding file from tar: {name} ({compression:?})");

				let entry = lookup.entry(name);
				let versions = entry.or_insert_with(|| FileEntry::new(mime.to_string()));
				match compression {
					Uncompressed => versions.un = Some(blob),
					Gzip => versions.gz = Some(blob),
					Brotli => versions.br = Some(blob),
				}
			};

			if filename == OsStr::new("index.html") {
				add(entry_path.parent().unwrap(), blob.clone());
			}
			add(&entry_path, blob);
		}

		Ok(Self {
			lookup,
			name: path.to_str().unwrap().to_owned(),
		})
	}
}

#[async_trait]
impl StaticSourceTrait for TarFile {
	#[cfg(test)]
	fn get_type(&self) -> &str {
		"tar"
	}

	#[cfg(test)]
	fn get_name(&self) -> &str {
		&self.name
	}

	fn get_data(&self, url: &Url, accept: &TargetCompression) -> Option<SourceResponse> {
		use TileCompression::*;

		let file_entry = self.lookup.get(&url.str[1..])?.to_owned();

		if accept.contains(Brotli)
			&& let Some(blob) = &file_entry.br
		{
			return SourceResponse::new_some(blob.to_owned(), Brotli, &file_entry.mime);
		}

		if accept.contains(Gzip)
			&& let Some(blob) = &file_entry.gz
		{
			return SourceResponse::new_some(blob.to_owned(), Gzip, &file_entry.mime);
		}

		if let Some(blob) = &file_entry.un {
			return SourceResponse::new_some(blob.to_owned(), Uncompressed, &file_entry.mime);
		}

		if let Some(blob) = &file_entry.br {
			return SourceResponse::new_some(blob.to_owned(), Brotli, &file_entry.mime);
		}

		if let Some(blob) = &file_entry.gz {
			return SourceResponse::new_some(blob.to_owned(), Gzip, &file_entry.mime);
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
	use assert_fs::NamedTempFile;
	use rstest::rstest;
	use versatiles_container::{
		MockTilesReader, MockTilesReaderProfile, TileSourceTrait, TilesConverterParameters, TilesRuntime,
		convert_tiles_container,
	};
	use versatiles_core::TileBBoxPyramid;

	pub async fn make_test_tar(compression: TileCompression) -> NamedTempFile {
		// get dummy reader
		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Pbf).unwrap();

		// get to test container converter
		let container_file = NamedTempFile::new("temp.tar").unwrap();

		let parameters = TilesConverterParameters {
			tile_compression: Some(compression),
			bbox_pyramid: Some(TileBBoxPyramid::new_full(0)),
			..TilesConverterParameters::default()
		};
		let runtime = TilesRuntime::default();

		convert_tiles_container(reader.boxed(), parameters, &container_file, runtime)
			.await
			.unwrap();

		container_file
	}

	#[tokio::test]
	async fn small_stuff() {
		let file = make_test_tar(TileCompression::Uncompressed).await;

		let tar_file = TarFile::from(&file).unwrap();

		assert!(tar_file.get_name().ends_with("temp.tar"));
		assert!(format!("{tar_file:?}").starts_with("TarFile { name:"));
	}

	#[test]
	fn from_non_existing_path() {
		let path = Path::new("path/to/non-existing/file.tar");
		assert!(TarFile::from(path).is_err());
	}

	#[test]
	fn from_directory() {
		let path = Path::new(".");
		assert!(TarFile::from(path).is_err());
	}

	#[rstest]
	#[case(TileCompression::Uncompressed)]
	#[case(TileCompression::Gzip)]
	#[case(TileCompression::Brotli)]
	#[tokio::test]
	async fn test_get_data(#[case] compression_tar: TileCompression) -> Result<()> {
		let file = make_test_tar(compression_tar).await;
		let mut tar_file = TarFile::from(&file)?;

		test2(&mut tar_file, compression_tar, TileCompression::Uncompressed)?;
		test2(&mut tar_file, compression_tar, TileCompression::Gzip)?;
		test2(&mut tar_file, compression_tar, TileCompression::Brotli)?;

		return Ok(());

		fn test2(
			tar_file: &mut TarFile,
			compression_tar: TileCompression,
			compression_accept: TileCompression,
		) -> Result<()> {
			let accept = TargetCompression::from(compression_accept);

			let result = tar_file.get_data(&Url::from("non_existing_file"), &accept);
			assert!(result.is_none());

			//let path = ["0", "0", "0"];
			let result = tar_file.get_data(&Url::from("tiles.json"), &accept);
			assert!(result.is_some());

			let result = result.unwrap();

			if result.compression == TileCompression::Uncompressed {
				assert_eq!(
					result.blob.as_str(),
					"{\"tile_format\":\"vnd.mapbox-vector-tile\",\"tile_schema\":\"other\",\"tile_type\":\"vector\",\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
				);
			}

			assert_eq!(result.mime, "application/json");
			assert_eq!(result.compression, compression_tar);

			Ok(())
		}
	}
}
