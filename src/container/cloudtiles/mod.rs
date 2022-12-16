use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, Write};

use crate::container::container;
use brotli::enc::BrotliEncoderParams;
use brotli::BrotliCompress;
use std::os::unix::fs::FileExt;

pub struct Reader;
impl container::Reader for Reader {}

pub struct Converter {
	tile_compression_dst: container::TileCompression,
	tile_recompress: bool,
}
impl container::Converter for Converter {
	fn convert_from(
		filename: &std::path::PathBuf,
		container: Box<dyn container::Reader>,
	) -> std::io::Result<()> {
		let mut converter = Converter {
			tile_compression_dst: container::TileCompression::None,
			tile_recompress: false,
		};

		let file = File::create(filename).expect("Unable to create file");
		let mut file = BufWriter::new(file);

		// magic word
		file.write(b"OpenCloudTiles")?;

		// version
		file.write(&[0])?;

		// format;
		let tile_type = container.get_tile_type();
		let tile_compression_src = container.get_tile_compression();

		let tile_type_value: u8 = match tile_type {
			container::TileType::PBF => {
				converter.tile_compression_dst = container::TileCompression::Brotli;
				0
			}
			container::TileType::PNG => 1,
			container::TileType::JPG => 2,
			container::TileType::WEBP => 3,
		};
		converter.tile_recompress = tile_compression_src != converter.tile_compression_dst;

		file.write(&[tile_type_value])?;

		let header_length = file.stream_position()?;

		// skip start and length of meta_blob and root_block
		file.write(&[0u8, 2 * 8])?;

		let mut metablob = container.get_meta().to_vec();
		let meta_blob_range = converter.write_compressed_brotli(&mut file, &mut metablob)?;
		let root_index_range = converter.write_rootdata(&mut file, &container)?;

		file.flush()?;

		let file = file.get_mut();

		meta_blob_range.write_at(file, header_length)?;
		root_index_range.write_at(file, header_length + 16)?;

		drop(file);

		return Ok(());
	}
}

impl Converter {
	fn write_compressed_brotli(
		&self,
		file: &mut BufWriter<File>,
		input: &Vec<u8>,
	) -> std::io::Result<ByteRange> {
		let params = &BrotliEncoderParams::default();
		let mut cursor = Cursor::new(input);
		let range = ByteRange::new(
			file.stream_position()?,
			BrotliCompress(&mut cursor, file, params)? as u64,
		);
		return Ok(range);
	}
	fn write_rootdata(
		&self,
		file: &mut BufWriter<File>,
		container: &Box<dyn container::Reader>,
	) -> std::io::Result<ByteRange> {
		let minimum_level = container.get_minimum_level();
		let maximum_level = container.get_maximum_level();
		let mut level_index: Vec<ByteRange> = Vec::new();

		for level in minimum_level..=maximum_level {
			level_index.push(self.write_level(file, container, level)?)
		}

		let level_index_start = file.stream_position()?;
		file.write(&minimum_level.to_le_bytes())?;
		file.write(&maximum_level.to_le_bytes())?;

		for range in level_index {
			range.write(file)?;
		}
		let level_index_end = file.stream_position()?;

		return Ok(ByteRange::new(
			level_index_start,
			level_index_end - level_index_start,
		));
	}
	fn write_level(
		&self,
		file: &mut BufWriter<File>,
		container: &Box<dyn container::Reader>,
		level: u64,
	) -> std::io::Result<ByteRange> {
		let minimum_row = container.get_minimum_row(level);
		let maximum_row = container.get_maximum_row(level);
		let minimum_col = container.get_minimum_col(level);
		let maximum_col = container.get_maximum_col(level);

		let mut row_index: Vec<ByteRange> = Vec::new();

		for row in minimum_row..=maximum_row {
			row_index.push(self.write_row(file, container, level, row)?);
		}

		let index_start = file.stream_position()?;
		file.write(&minimum_col.to_le_bytes())?;
		file.write(&maximum_col.to_le_bytes())?;
		file.write(&minimum_row.to_le_bytes())?;
		file.write(&maximum_row.to_le_bytes())?;

		for range in row_index {
			range.write(file)?;
		}
		let index_end = file.stream_position()?;

		return Ok(ByteRange::new(index_start, index_end - index_start));
	}
	fn write_row(
		&self,
		file: &mut BufWriter<File>,
		container: &Box<dyn container::Reader>,
		level: u64,
		row: u64,
	) -> std::io::Result<ByteRange> {
		let minimum_col = container.get_minimum_col(level);
		let maximum_col = container.get_maximum_col(level);

		let mut tile_index: Vec<ByteRange> = Vec::new();

		for col in minimum_col..=maximum_col {
			tile_index.push(self.write_tile(file, container, level, row, col)?);
		}

		let index_start = file.stream_position()?;
		for range in tile_index {
			range.write(file)?;
		}
		let index_end = file.stream_position()?;

		return Ok(ByteRange::new(index_start, index_end - index_start));
	}
	fn write_tile(
		&self,
		file: &mut BufWriter<File>,
		container: &Box<dyn container::Reader>,
		level: u64,
		row: u64,
		col: u64,
	) -> std::io::Result<ByteRange> {
		if self.tile_recompress {
			let tile = container.get_tile_uncompressed(level, row, col)?;
			let range = self.write_compressed_brotli(file, &tile)?;
			return Ok(range);
		} else {
			let tile = container.get_tile_raw(level, row, col)?;

			let tile_start = file.stream_position()?;
			file.write(&tile)?;
			let tile_end = file.stream_position()?;

			return Ok(ByteRange::new(tile_start, tile_end - tile_start));
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
		return Ok(());
	}
	fn write(self, file: &mut BufWriter<File>) -> std::io::Result<()> {
		file.write(&(self.offset as u64).to_le_bytes())?;
		file.write(&(self.length as u64).to_le_bytes())?;
		return Ok(());
	}
}
