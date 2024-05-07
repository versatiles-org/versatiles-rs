use super::*;
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

pub async fn get_writer(filename: &str, config: TilesWriterParameters) -> Result<TilesWriterBox> {
	let path = env::current_dir()?.join(filename);

	let extension = get_extension(&path);
	match extension.as_str() {
		"versatiles" => VersaTilesWriter::open_file(&path, config).await,
		"tar" => TarTilesWriter::open_file(&path, config),
		"" => DirectoryTilesWriter::open_file(&path, config),
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

#[allow(unused_imports)]
#[cfg(test)]
pub mod tests {
	use crate::{
		containers::{
			get_reader, get_writer, MockTilesReader, MockTilesReaderProfile as RP, MockTilesWriter,
			MockTilesWriterProfile as CP, TilesReaderParameters,
		},
		shared::{Compression as C, TileBBoxPyramid, TileFormat as TF},
	};
	use anyhow::Result;
	use assert_fs::fixture::NamedTempFile;
	use std::time::Instant;

	use super::writer::TilesWriterParameters;

	pub async fn make_test_file(
		tile_format: TF, compression: C, max_zoom_level: u8, extension: &str,
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

		let config = TilesWriterParameters::new(tile_format, compression);
		let mut writer = get_writer(container_file.to_str().unwrap(), config).await?;

		// convert
		writer.write_tiles(&mut reader).await?;

		Ok(container_file)
	}

	#[test]
	fn writers_and_readers() -> Result<()> {
		#[derive(Debug)]
		enum Container {
			Tar,
			Versatiles,
		}

		#[tokio::main]
		async fn test_writer_and_reader(container: &Container, tile_format: TF, compression: C) -> Result<()> {
			let _test_name = format!("{:?}, {:?}, {:?}", container, tile_format, compression);

			let _start = Instant::now();

			// get dummy reader
			let mut reader1 = MockTilesReader::new_mock(TilesReaderParameters::new(
				tile_format,
				compression,
				TileBBoxPyramid::new_full(4),
			));

			// get to test container comverter
			let container_file = match container {
				Container::Tar => NamedTempFile::new("temp.tar"),
				Container::Versatiles => NamedTempFile::new("temp.versatiles"),
			}?;

			let config = TilesWriterParameters::new(tile_format, compression);
			let mut converter1 = get_writer(container_file.to_str().unwrap(), config).await?;

			// convert
			converter1.write_tiles(&mut reader1).await?;

			// get test container reader
			let mut reader2 = get_reader(container_file.to_str().unwrap()).await?;
			let mut converter2 = MockTilesWriter::new_mock(TilesWriterParameters::new(tile_format, compression));
			converter2.write_tiles(&mut reader2).await?;

			Ok(())
		}

		let containers = vec![Container::Tar, Container::Versatiles];

		for container in containers {
			test_writer_and_reader(&container, TF::PNG, C::None)?;
			test_writer_and_reader(&container, TF::JPG, C::None)?;
			test_writer_and_reader(&container, TF::PBF, C::Gzip)?;
		}

		Ok(())
	}
}
