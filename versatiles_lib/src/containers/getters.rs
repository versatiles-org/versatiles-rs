use super::mbtiles;
use super::tar;
use super::{versatiles, TileReaderBox, TileReaderTrait};
use super::{TileConverterBox, TileConverterTrait};
use crate::shared::TileConverterConfig;
use crate::{create_error, shared::Result};
use std::path::{Path, PathBuf};

pub async fn get_reader(filename: &str) -> Result<TileReaderBox> {
	let extension = get_extension(&PathBuf::from(filename));
	match extension.as_str() {
		"mbtiles" => mbtiles::TileReader::new(filename).await,
		"tar" => tar::TileReader::new(filename).await,
		"versatiles" => versatiles::TileReader::new(filename).await,
		_ => create_error!("Error when reading: file extension '{extension:?}' unknown"),
	}
}

pub async fn get_converter(filename: &str, config: TileConverterConfig) -> Result<TileConverterBox> {
	let path = PathBuf::from(filename);
	let extension = get_extension(&path);
	match extension.as_str() {
		"versatiles" => versatiles::TileConverter::new(filename, config).await,
		"tar" => tar::TileConverter::new(filename, config).await,
		_ => create_error!("Error when writing: file extension '{extension:?}' unknown"),
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
		containers::get_reader,
		shared::{Compression as C, Result, TileBBoxPyramid, TileFormat as TF},
	};

	use crate::{
		containers::{
			dummy::{self, ConverterProfile as CP, ReaderProfile as RP},
			get_converter,
		},
		shared::TileConverterConfig,
	};
	use assert_fs::fixture::NamedTempFile;
	use std::time::Instant;

	pub async fn make_test_file(
		tile_format: TF, compression: C, max_zoom_level: u8, extension: &str,
	) -> Result<NamedTempFile> {
		let reader_profile = match tile_format {
			TF::PNG => RP::PNG,
			TF::JPG => RP::PNG,
			TF::WEBP => RP::PNG,
			TF::PBF => RP::PBF,
			_ => todo!(),
		};

		// get dummy reader
		let mut reader = dummy::TileReader::new_dummy(reader_profile, max_zoom_level);

		// get to test container comverter
		let container_file = match extension {
			"tar" => NamedTempFile::new("temp.tar"),
			"versatiles" => NamedTempFile::new("temp.versatiles"),
			_ => panic!("make_test_file: extension {extension} not found"),
		}?;

		let config = TileConverterConfig::new(Some(tile_format), Some(compression), TileBBoxPyramid::new_full(), false);
		let mut converter = get_converter(container_file.to_str().unwrap(), config).await?;

		// convert
		converter.convert_from(&mut reader).await?;

		Ok(container_file)
	}

	#[test]

	fn converters_and_readers() -> Result<()> {
		#[derive(Debug)]
		enum Container {
			Tar,
			Versatiles,
		}

		#[tokio::main]
		async fn test_converter_and_reader(
			reader_profile: RP, max_zoom_level: u8, container: &Container, tile_format: TF, compression: C,
			force_recompress: bool,
		) -> Result<()> {
			let _test_name = format!(
				"{:?}, {}, {:?}, {:?}, {:?}, {:?}",
				reader_profile, max_zoom_level, container, tile_format, compression, force_recompress
			);

			let _start = Instant::now();

			// get dummy reader
			let mut reader1 = dummy::TileReader::new_dummy(reader_profile, max_zoom_level);

			// get to test container comverter
			let container_file = match container {
				Container::Tar => NamedTempFile::new("temp.tar"),
				Container::Versatiles => NamedTempFile::new("temp.versatiles"),
			}?;

			let config = TileConverterConfig::new(
				Some(tile_format),
				Some(compression),
				TileBBoxPyramid::new_full(),
				force_recompress,
			);
			let mut converter1 = get_converter(container_file.to_str().unwrap(), config).await?;

			// convert
			converter1.convert_from(&mut reader1).await?;

			// get test container reader
			let mut reader2 = get_reader(container_file.to_str().unwrap()).await?;
			let mut converter2 = dummy::TileConverter::new_dummy(CP::Whatever, max_zoom_level);
			converter2.convert_from(&mut reader2).await?;

			Ok(())
		}

		let containers = vec![Container::Tar, Container::Versatiles];

		for container in containers {
			test_converter_and_reader(RP::PNG, 7, &container, TF::PNG, C::None, false)?;
			test_converter_and_reader(RP::PNG, 4, &container, TF::JPG, C::None, false)?;
			test_converter_and_reader(RP::PBF, 7, &container, TF::PBF, C::Gzip, false)?;
		}

		Ok(())
	}
}
