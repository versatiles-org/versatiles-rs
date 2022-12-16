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
		let metablob_range = write_compressed_brotli(&mut metablob, &mut file)?;
		let rootindex_range = write_rootdata(&mut file)?;

		file.flush()?;

		let file = file.get_mut();

		metablob_range.write_at(file, header_length);
		rootindex_range.write_at(file, header_length + 16);

		drop(file);
		//file.write(buf)
		panic!("not implemented");

		fn write_compressed_brotli(
			input: &mut Vec<u8>,
			file: &mut BufWriter<File>,
		) -> std::io::Result<ByteRange> {
			let params = &BrotliEncoderParams::default();
			let mut cursor = Cursor::new(input);
			let range = ByteRange::new(
				file.stream_position()?,
				BrotliCompress(&mut cursor, file, params)? as u64,
			);
			return Ok(range);
		}
		fn write_rootdata(file: &mut BufWriter<File>) -> std::io::Result<ByteRange> {
			let range = ByteRange::new(file.stream_position()?, 0);
			return Ok(range);
		}
	}
}

struct ByteRange {
	offset: u64,
	length: u64,
}

impl ByteRange {
	fn new(offset: u64, length: u64) -> ByteRange {
		return ByteRange { offset, length };
	}
	fn write_at(self, file: &mut File, pos: u64) -> std::io::Result<()> {
		file.write_at(&(self.offset as u64).to_le_bytes(), pos)?;
		file.write_at(&(self.length as u64).to_le_bytes(), pos + 8)?;
		Ok(())
	}
}
