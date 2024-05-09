use super::{
	DirectoryTilesReader, DirectoryTilesWriter, MBTilesReader, TarTilesReader, TarTilesWriter, TilesReaderBox,
	TilesWriterBox, TilesWriterParameters, VersaTilesReader, VersaTilesWriter,
};
use anyhow::{bail, Context, Result};
use reqwest::Url;
use std::{env, path::Path};

pub async fn get_reader(filename: &str) -> Result<TilesReaderBox> {
	let path = env::current_dir()?.join(filename);

	if filename.starts_with("http://") || filename.starts_with("https://") {
		let url = Url::parse(filename)?;
		let extension = get_extension(&path);
		return match extension.as_str() {
			"versatiles" => VersaTilesReader::open_url(url).await,
			_ => bail!("Error when reading: file extension '{extension:?}' unknown"),
		};
	}

	if path.is_dir() {
		return DirectoryTilesReader::open(&path)
			.await
			.with_context(|| format!("opening {path:?} as directory"));
	}

	let extension = get_extension(&path);
	match extension.as_str() {
		"mbtiles" => MBTilesReader::open(&path).await,
		"tar" => TarTilesReader::open(&path).await,
		"versatiles" => VersaTilesReader::open_file(&path).await,
		_ => bail!("Error when reading: file extension '{extension:?}' unknown"),
	}
}

pub async fn get_writer(filename: &str, parameters: TilesWriterParameters) -> Result<TilesWriterBox> {
	let path = env::current_dir()?.join(filename);

	let extension = get_extension(&path);
	match extension.as_str() {
		"versatiles" => VersaTilesWriter::open_file(&path, parameters).await,
		"tar" => TarTilesWriter::open_file(&path, parameters),
		"" => DirectoryTilesWriter::open_file(&path, parameters),
		_ => bail!("Error when writing: file extension '{extension:?}' unknown"),
	}
}

fn get_extension(path: &Path) -> String {
	if let Some(osstr) = path.extension() {
		String::from(osstr.to_str().unwrap_or(""))
	} else {
		String::from("")
	}
}

#[cfg(test)]
pub mod tests {
	use super::*;
	use crate::{
		container::{MockTilesReader, MockTilesWriter, TilesReaderParameters},
		types::{TileBBoxPyramid, TileCompression, TileFormat},
	};
	use anyhow::Result;
	use assert_fs::{fixture::NamedTempFile, TempDir};
	use std::time::Instant;

	pub async fn make_test_file(
		tile_format: TileFormat, compression: TileCompression, max_zoom_level: u8, extension: &str,
	) -> Result<NamedTempFile> {
		// get dummy reader
		let mut reader = MockTilesReader::new_mock(TilesReaderParameters::new(
			tile_format,
			compression,
			TileBBoxPyramid::new_full(max_zoom_level),
		));

		// get to test container comverter
		let container_file = match extension {
			"tar" => NamedTempFile::new("temp.tar"),
			"versatiles" => NamedTempFile::new("temp.versatiles"),
			_ => panic!("make_test_file: extension {extension} not found"),
		}?;

		let parameters = TilesWriterParameters::new(tile_format, compression);
		let mut writer = get_writer(container_file.to_str().unwrap(), parameters).await?;

		// convert
		writer.write_tiles(&mut reader).await?;

		Ok(container_file)
	}

	#[test]
	fn writers_and_readers() -> Result<()> {
		#[derive(Debug)]
		enum Container {
			Directory,
			Tar,
			Versatiles,
		}

		#[tokio::main]
		async fn test_writer_and_reader(
			container: &Container, tile_format: TileFormat, compression: TileCompression,
		) -> Result<()> {
			let _test_name = format!("{:?}, {:?}, {:?}", container, tile_format, compression);

			let _start = Instant::now();

			// get dummy reader
			let mut reader1 = MockTilesReader::new_mock(TilesReaderParameters::new(
				tile_format,
				compression,
				TileBBoxPyramid::new_full(2),
			));

			enum TempType {
				Dir(TempDir),
				File(NamedTempFile),
			}

			// get to test container comverter
			let path: TempType = match container {
				Container::Directory => TempType::Dir(TempDir::new()?),
				Container::Tar => TempType::File(NamedTempFile::new("temp.tar")?),
				Container::Versatiles => TempType::File(NamedTempFile::new("temp.versatiles")?),
			};

			let filename = match &path {
				TempType::Dir(t) => t.to_str().unwrap(),
				TempType::File(t) => t.to_str().unwrap(),
			};

			let parameters = TilesWriterParameters::new(tile_format, compression);
			let mut writer1 = get_writer(filename, parameters).await?;

			// convert
			writer1.write_from_reader(&mut reader1).await?;

			// get test container reader
			let mut reader2 = get_reader(filename).await?;
			let mut writer2 = MockTilesWriter::new_mock(TilesWriterParameters::new(tile_format, compression));
			writer2.write_from_reader(&mut reader2).await?;

			Ok(())
		}

		let containers = vec![Container::Directory, Container::Tar, Container::Versatiles];

		for container in containers {
			test_writer_and_reader(&container, TileFormat::PNG, TileCompression::None)?;
			test_writer_and_reader(&container, TileFormat::JPG, TileCompression::None)?;
			test_writer_and_reader(&container, TileFormat::PBF, TileCompression::Gzip)?;
		}

		Ok(())
	}
}
