pub mod dummy;
pub mod mbtiles;
pub mod tar_file;
pub mod versatiles;

mod traits;
pub use traits::*;

use std::path::PathBuf;
use versatiles_shared::{Result, TileConverterConfig};

pub async fn get_reader(filename: &str) -> Result<TileReaderBox> {
	let extension = filename.split('.').last().unwrap();

	let reader = match extension {
		"mbtiles" => mbtiles::TileReader::new(filename),
		"tar" => tar_file::TileReader::new(filename),
		"versatiles" => versatiles::TileReader::new(filename),
		_ => panic!("extension '{extension:?}' unknown"),
	};

	reader.await
}

pub fn get_converter(filename: &str, config: TileConverterConfig) -> TileConverterBox {
	let path = PathBuf::from(filename);
	let extension = path.extension().unwrap().to_str().expect("file has no extension");

	let converter = match extension {
		//"mbtiles" => mbtiles::TileConverter::new(&path, config),
		"versatiles" => versatiles::TileConverter::new(&path, config),
		"tar" => tar_file::TileConverter::new(&path, config),
		_ => panic!("extension '{extension:?}' unknown"),
	};
	converter
}

#[cfg(test)]
mod tests {
	use crate::{
		dummy::{self, ConverterProfile, ReaderProfile},
		get_converter, get_reader,
	};
	use assert_fs::fixture::NamedTempFile;
	use std::time::Instant;
	use versatiles_shared::{Precompression, TileBBoxPyramide, TileConverterConfig, TileFormat};

	#[test]
	fn test_converters() {
		/*
		#[tokio::main]
		async fn test(extension: &str, compression: Precompression, force_recompress: bool) {
			println!("test {:?}, {:?}, {:?}", extension, compression, force_recompress);

			let start = Instant::now();

			let mut bbox_pyramide = TileBBoxPyramide::new_full();

			// ensure test duration of < 100 ms
			match compression {
				Precompression::Uncompressed => bbox_pyramide.set_zoom_max(13),
				Precompression::Gzip => bbox_pyramide.set_zoom_max(12),
				Precompression::Brotli => bbox_pyramide.set_zoom_max(6),
			};

			let config = TileConverterConfig::new(None, Some(compression), bbox_pyramide, force_recompress);
			let tmp_file = NamedTempFile::new("temp.".to_owned() + extension).unwrap();
			let mut reader = TileReader::new("../../../ressources/berlin.mbtiles").await.unwrap();
			let mut convert = get_converter(tmp_file.to_str().unwrap(), config);
			convert.convert_from(&mut reader).await;
			tmp_file.close().unwrap();

			let duration = start.elapsed();
			println!("Time elapsed in expensive_function() is: {:?}", duration);
		}

		let extensions = ["tar", "versatiles"];
		for extension in extensions {
			test(extension, Precompression::Uncompressed, true);
			test(extension, Precompression::Uncompressed, false);
			test(extension, Precompression::Gzip, true);
			test(extension, Precompression::Gzip, false);
			test(extension, Precompression::Brotli, true);
			test(extension, Precompression::Brotli, false);
		}
		 */
	}
	#[test]
	fn test_readers() {
		#[derive(Debug)]
		enum Container {
			Tar,
			Versatiles,
		}

		#[tokio::main]
		async fn test(
			reader_profile: ReaderProfile, max_zoom_level: u8, container: &Container, tile_format: TileFormat,
			compression: Precompression, force_recompress: bool,
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
				false,
			);
			let mut converter1 = get_converter(&container_file.to_str().unwrap(), config);

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
				Precompression::Uncompressed,
				false,
			);
			test(
				ReaderProfile::PngFast,
				4,
				&container,
				TileFormat::JPG,
				Precompression::Uncompressed,
				false,
			);
			test(
				ReaderProfile::PbfFast,
				7,
				&container,
				TileFormat::PBF,
				Precompression::Gzip,
				false,
			);
		}
	}
}
