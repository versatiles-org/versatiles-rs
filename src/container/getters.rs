use crate::{
	container::{
		directory::{DirectoryTilesReader, DirectoryTilesWriter},
		mbtiles::MBTilesReader,
		pmtiles::{PMTilesReader, PMTilesWriter},
		tar::{TarTilesReader, TarTilesWriter},
		versatiles::{VersaTilesReader, VersaTilesWriter},
		TilesReader, TilesWriter,
	},
	types::{DataReader, DataReaderHttp},
};
use anyhow::{bail, Context, Result};
use reqwest::Url;
use std::env;

pub async fn get_reader(filename: &str) -> Result<Box<dyn TilesReader>> {
	let extension = get_extension(filename);

	if let Ok(reader) = parse_as_url(filename) {
		match extension {
			"pmtiles" => return Ok(PMTilesReader::open_reader(reader).await?.boxed()),
			"versatiles" => return Ok(VersaTilesReader::open_reader(reader).await?.boxed()),
			_ => bail!("Error when reading: file extension '{extension:?}' unknown"),
		}
	}

	let path = env::current_dir()?.join(filename);

	if !path.exists() {
		bail!("path '{path:?}' does not exist")
	}

	if path.is_dir() {
		return Ok(DirectoryTilesReader::open_path(&path)
			.with_context(|| format!("opening {path:?} as directory"))?
			.boxed());
	}

	match extension {
		"mbtiles" => Ok(MBTilesReader::open_path(&path)?.boxed()),
		"pmtiles" => Ok(PMTilesReader::open_path(&path).await?.boxed()),
		"tar" => Ok(TarTilesReader::open_path(&path)?.boxed()),
		"versatiles" => Ok(VersaTilesReader::open_path(&path).await?.boxed()),
		_ => bail!("Error when reading: file extension '{extension:?}' unknown"),
	}
}

fn parse_as_url(filename: &str) -> Result<DataReader> {
	if filename.starts_with("http://") || filename.starts_with("https://") {
		Ok(DataReaderHttp::from_url(Url::parse(filename)?)?)
	} else {
		bail!("not an url")
	}
}

pub async fn write_to_filename(reader: &mut dyn TilesReader, filename: &str) -> Result<()> {
	let path = env::current_dir()?.join(filename);

	if path.is_dir() {
		return DirectoryTilesWriter::write_to_path(reader, &path).await;
	}

	let extension = get_extension(filename);
	match extension {
		"tar" => TarTilesWriter::write_to_path(reader, &path).await,
		"pmtiles" => PMTilesWriter::write_to_path(reader, &path).await,
		"versatiles" => VersaTilesWriter::write_to_path(reader, &path).await,
		_ => bail!("Error when writing: file extension '{extension:?}' unknown"),
	}
}

fn get_extension(filename: &str) -> &str {
	filename
		.split('?')
		.next()
		.map(|filename| filename.rsplit('.').next().unwrap_or(""))
		.unwrap_or("")
}

#[cfg(test)]
pub mod tests {
	use super::*;
	use crate::{
		container::{
			mock::{MockTilesReader, MockTilesWriter},
			TilesReaderParameters,
		},
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
		))?;

		// get to test container comverter
		let container_file = match extension {
			"tar" => NamedTempFile::new("temp.tar"),
			"versatiles" => NamedTempFile::new("temp.versatiles"),
			_ => panic!("make_test_file: extension {extension} not found"),
		}?;

		write_to_filename(&mut reader, container_file.to_str().unwrap()).await?;

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
			))?;

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

			write_to_filename(&mut reader1, filename).await?;

			// get test container reader
			let mut reader2 = get_reader(filename).await?;
			MockTilesWriter::write(reader2.as_mut()).await?;

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
