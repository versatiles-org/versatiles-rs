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
	use crate::{get_converter, mbtiles::TileReader, TileReaderTrait};
	use assert_fs::fixture::NamedTempFile;
	use std::time::Instant;
	use versatiles_shared::{Precompression, TileBBoxPyramide, TileConverterConfig};

	#[test]
	fn test_converters() {
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
	}
}
