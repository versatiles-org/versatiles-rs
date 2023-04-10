use super::{mbtiles, tar, versatiles, TileConverterBox, TileConverterTrait, TileReaderBox, TileReaderTrait};
use crate::shared::{Error, Result, TileConverterConfig};
use log::error;
use std::path::PathBuf;

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

pub fn get_converter(filename: &str, config: TileConverterConfig) -> Result<TileConverterBox> {
	let path = PathBuf::from(filename);
	let extension = get_extension(&path);
	match extension.as_str() {
		"versatiles" => Ok(versatiles::TileConverter::new(&path, config)),
		"tar" => Ok(tar::TileConverter::new(&path, config)),
		_ => {
			error!("Error when writing: file extension '{extension:?}' unknown");
			Err(Error::new("file extension unknown"))
		}
	}
}

fn get_extension(path: &PathBuf) -> String {
	if let Some(osstr) = path.extension() {
		return String::from(osstr.to_str().unwrap_or(""));
	} else {
		return String::from("");
	}
}

#[cfg(test)]
pub mod tests {
	use crate::{
		containers::{
			dummy::{self, ConverterProfile, ReaderProfile},
			get_converter, get_reader,
		},
		shared::{Compression, TileBBoxPyramide, TileConverterConfig, TileFormat},
	};
	use assert_fs::fixture::NamedTempFile;
	use std::time::Instant;

	pub async fn make_test_file(
		tile_format: TileFormat, compression: Compression, max_zoom_level: u8, extension: &str,
	) -> NamedTempFile {
		let reader_profile = match tile_format {
			TileFormat::PNG => ReaderProfile::PngFast,
			TileFormat::JPG => ReaderProfile::PngFast,
			TileFormat::WEBP => ReaderProfile::PngFast,
			TileFormat::PBF => ReaderProfile::PbfFast,
			_ => todo!(),
		};

		// get dummy reader
		let mut reader = dummy::TileReader::new_dummy(reader_profile, max_zoom_level);

		// get to test container comverter
		let container_file = match extension {
			"tar" => NamedTempFile::new("temp.tar"),
			"versatiles" => NamedTempFile::new("temp.versatiles"),
			_ => panic!(),
		}
		.unwrap();

		let config = TileConverterConfig::new(
			Some(tile_format),
			Some(compression),
			TileBBoxPyramide::new_full(),
			false,
		);
		let mut converter = get_converter(&container_file.to_str().unwrap(), config).unwrap();

		// convert
		converter.convert_from(&mut reader).await;

		container_file
	}

	#[test]
	fn converters_and_readers() {
		#[derive(Debug)]
		enum Container {
			Tar,
			Versatiles,
		}

		#[tokio::main]
		async fn test(
			reader_profile: ReaderProfile, max_zoom_level: u8, container: &Container, tile_format: TileFormat,
			compression: Compression, force_recompress: bool,
		) {
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
			}
			.unwrap();

			let config = TileConverterConfig::new(
				Some(tile_format),
				Some(compression),
				TileBBoxPyramide::new_full(),
				force_recompress,
			);
			let mut converter1 = get_converter(&container_file.to_str().unwrap(), config).unwrap();

			// convert
			converter1.convert_from(&mut reader1).await;

			// get test container reader
			let mut reader2 = get_reader(container_file.to_str().unwrap()).await.unwrap();
			let mut converter2 = dummy::TileConverter::new_dummy(ConverterProfile::Whatever, max_zoom_level);
			converter2.convert_from(&mut reader2).await;

			println!("elapsed time for {}: {:?}", test_name, start.elapsed());
		}

		let containers = vec![Container::Tar, Container::Versatiles];

		for container in containers {
			test(
				ReaderProfile::PngFast,
				7,
				&container,
				TileFormat::PNG,
				Compression::None,
				false,
			);
			test(
				ReaderProfile::PngFast,
				4,
				&container,
				TileFormat::JPG,
				Compression::None,
				false,
			);
			test(
				ReaderProfile::PbfFast,
				7,
				&container,
				TileFormat::PBF,
				Compression::Gzip,
				false,
			);
		}
	}
}
