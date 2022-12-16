use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, Write};

use crate::container::container;
use brotli::enc::BrotliEncoderParams;
use brotli::BrotliCompress;
use std::os::unix::fs::FileExt;

pub struct Reader;
impl container::Reader for Reader {}

pub struct Converter;
impl container::Converter for Converter {
	fn convert_from(
		filename: &std::path::PathBuf,
		container: Box<dyn container::Reader>,
	) -> std::io::Result<()> {
		let file = File::create(filename).expect("Unable to create file");
		let mut file = BufWriter::new(file);

		// magic word
		file.write(b"OpenCloudTiles")?;

		// version
		file.write(&[0])?;

		// format;
		let tile_type = container.get_tile_type();
		let tile_compression_src = container.get_tile_compression();
		let mut tile_compression_dst = container::TileCompression::None;
		let tile_type_value: u8 = match tile_type {
			container::TileType::PBF => {
				tile_compression_dst = container::TileCompression::Brotli;
				0
			}
			container::TileType::PNG => 1,
			container::TileType::JPG => 2,
			container::TileType::WEBP => 3,
		};
		file.write(&[tile_type_value])?;

		let header_length = file.stream_position()?;

		// skip start and length of meta_blob and root_block
		file.write(&[0u8, 2 * 8]);

		let mut metablob = container.get_meta();
		let metablob_length = write_compressed_brotli(&mut metablob, &mut file)?;
		let rootblock_length = write_rootdata(&mut file)?;

		file.flush()?;

		let file = file.get_mut();

		file.write_at(&(metablob_length as u64).to_le_bytes(), header_length)?;
		file.write_at(&(rootblock_length as u64).to_le_bytes(), header_length + 8)?;

		drop(file);
		//file.write(buf)
		panic!("not implemented");

		fn write_compressed_brotli(
			input: &mut Vec<u8>,
			file: &mut BufWriter<File>,
		) -> std::io::Result<usize> {
			let params = &BrotliEncoderParams::default();
			let mut cursor = Cursor::new(input);
			return BrotliCompress(&mut cursor, file, params);
		}
		fn write_rootdata(file: &mut BufWriter<File>) -> std::io::Result<usize> {
			return Ok(0);
		}
	}
}
