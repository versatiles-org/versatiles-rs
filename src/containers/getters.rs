use super::{mbtiles, tar, versatiles, TileConverterBox, TileConverterTrait, TileReaderBox, TileReaderTrait};
use crate::shared::{Error, Result, TileConverterConfig};
use log::error;
use std::path::{Path, PathBuf};

pub async fn get_reader(filename: &str) -> Result<TileReaderBox> {
	let extension = get_extension(&PathBuf::from(filename));
	match extension.as_str() {
		"mbtiles" => mbtiles::TileReader::new(filename).await,
		"tar" => tar::TileReader::new(filename).await,
		"versatiles" => versatiles::TileReader::new(filename).await,
		_ => {
			error!("Error when reading: file extension '{extension:?}' unknown");
			Err(Error::new("file extension unknown"))
		}
	}
}

pub async fn get_converter(filename: &str, config: TileConverterConfig) -> Result<TileConverterBox> {
	let path = PathBuf::from(filename);
	let extension = get_extension(&path);
	match extension.as_str() {
		"versatiles" => versatiles::TileConverter::new(filename, config).await,
		"tar" => tar::TileConverter::new(filename, config).await,
		_ => {
			error!("Error when writing: file extension '{extension:?}' unknown");
			Err(Error::new("file extension unknown"))
		}
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
	use crate::{
		containers::{
			dummy::{self, ConverterProfile as CP, ReaderProfile as RP},
			get_converter, get_reader,
		},
		shared::{Compression as C, Result, TileBBoxPyramid, TileConverterConfig, TileFormat as TF},
	};
	use assert_fs::fixture::NamedTempFile;
	use std::time::Instant;

	pub async fn make_test_file(
		tile_format: TF, compression: C, max_zoom_level: u8, extension: &str,
	) -> Result<NamedTempFile> {
		let reader_profile = match tile_format {
			TF::PNG => RP::PngFast,
			TF::JPG => RP::PngFast,
			TF::WEBP => RP::PngFast,
			TF::PBF => RP::PbfFast,
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
		let mut converter = get_converter(&container_file.to_str().unwrap(), config).await?;

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
		async fn test(
			reader_profile: RP, max_zoom_level: u8, container: &Container, tile_format: TF, compression: C,
			force_recompress: bool,
		) -> Result<()> {
			let test_name = format!(
				"{:?}, {}, {:?}, {:?}, {:?}, {:?}",
				reader_profile, max_zoom_level, container, tile_format, compression, force_recompress
			);
			println!("test: {}", test_name);

			let start = Instant::now();

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
			let mut converter1 = get_converter(&container_file.to_str().unwrap(), config).await?;

			// convert
			converter1.convert_from(&mut reader1).await?;

			// get test container reader
			let mut reader2 = get_reader(container_file.to_str().unwrap()).await?;
			let mut converter2 = dummy::TileConverter::new_dummy(CP::Whatever, max_zoom_level);
			converter2.convert_from(&mut reader2).await?;

			println!("elapsed time for {}: {:?}", test_name, start.elapsed());

			Ok(())
		}

		let containers = vec![Container::Tar, Container::Versatiles];

		for container in containers {
			test(RP::PngFast, 7, &container, TF::PNG, C::None, false)?;
			test(RP::PngFast, 4, &container, TF::JPG, C::None, false)?;
			test(RP::PbfFast, 7, &container, TF::PBF, C::Gzip, false)?;
		}
		Ok(())
	}
}
