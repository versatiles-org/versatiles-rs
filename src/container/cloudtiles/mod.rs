use super::container;
use super::container::{TileCompression, TileFormat};
use brotli::{enc::BrotliEncoderParams, BrotliCompress};
use flate2::{bufread::GzEncoder, Compression};
use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Seek, Write};
use std::os::unix::fs::FileExt;
use std::path::PathBuf;

pub struct Reader;
impl container::Reader for Reader {}

pub struct Converter {
	tile_compression: Option<TileCompression>,
	tile_recompress: bool,
	filename: PathBuf,
}
impl container::Converter for Converter {
	fn new(filename: &PathBuf) -> std::io::Result<Box<dyn container::Converter>>
	where
		Self: Sized,
	{
		Ok(Box::new(Converter {
			tile_compression: None,
			tile_recompress: false,
			filename: filename.clone(),
		}))
	}
	fn convert_from(&mut self, container: Box<dyn container::Reader>) -> std::io::Result<()> {
		let file = File::create(&self.filename).expect("Unable to create file");
		let mut file = BufWriter::new(file);

		// magic word
		file.write(b"OpenCloudTiles/Container")?;

		// version
		file.write(&[1])?;

		// format
		let tile_format = container.get_tile_format();
		let tile_format_value: u8 = match tile_format {
			TileFormat::PBF => 0,
			TileFormat::PNG => 1,
			TileFormat::JPG => 2,
			TileFormat::WEBP => 3,
		};
		file.write(&[tile_format_value])?;

		// precompression
		let tile_compression_src = container.get_tile_compression();

		if self.tile_compression.is_none() {
			self.tile_compression = Some(tile_compression_src.clone());
		}
		self.tile_recompress = self.tile_compression.as_ref().unwrap() != &tile_compression_src;
		let tile_compression_dst_value: u8 = match self.tile_compression {
			Some(TileCompression::None) => 0,
			Some(TileCompression::Gzip) => 1,
			Some(TileCompression::Brotli) => 2,
			None => panic!(),
		};
		file.write(&[tile_compression_dst_value])?;
		println!("tile_compression: {:?}", self.tile_compression);
		println!("tile_compression_src: {:?}", tile_compression_src);
		println!("tile_recompress: {}", self.tile_recompress);

		// add zeros
		file.write(&[0u8, 229])?;

		let mut metablob = container.get_meta().to_vec();
		let meta_blob_range = write_vec_brotli(&mut file, &mut metablob)?;
		let root_index_range = self.write_rootdata(&mut file, &container)?;

		file.flush()?;

		let file = file.get_mut();

		meta_blob_range.write_at(file, 128)?;
		root_index_range.write_at(file, 144)?;

		drop(file);

		return Ok(());
	}
	fn set_precompression(&mut self, compression: &TileCompression) {
		self.tile_compression = Some(compression.clone());
	}
}

impl Converter {
	fn write_rootdata(
		&self,
		file: &mut BufWriter<File>,
		container: &Box<dyn container::Reader>,
	) -> std::io::Result<ByteRange> {
		let minimum_level = container.get_minimum_zoom();
		let maximum_level = container.get_maximum_zoom();
		let mut level_index: Vec<ByteRange> = Vec::new();

		for level in minimum_level..=maximum_level {
			level_index.push(self.write_level(file, container, level)?)
		}

		let level_index_start = file.stream_position()?;
		file.write(&minimum_level.to_le_bytes())?;
		file.write(&maximum_level.to_le_bytes())?;

		write_compressed_index(file, level_index)?;
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
			row_index.push(self.write_row(file, container, level, row, minimum_col, maximum_col)?);
		}

		let index_start = file.stream_position()?;
		file.write(&minimum_col.to_le_bytes())?;
		file.write(&maximum_col.to_le_bytes())?;
		file.write(&minimum_row.to_le_bytes())?;
		file.write(&maximum_row.to_le_bytes())?;
		write_compressed_index(file, row_index)?;
		let index_end = file.stream_position()?;

		return Ok(ByteRange::new(index_start, index_end - index_start));
	}
	fn write_row(
		&self,
		file: &mut BufWriter<File>,
		container: &Box<dyn container::Reader>,
		level: u64,
		row: u64,
		minimum_col: u64,
		maximum_col: u64,
	) -> std::io::Result<ByteRange> {
		println!("{} / {} / x{}", level, row, maximum_col - minimum_col + 1);

		let mut tile_index: Vec<ByteRange> = Vec::new();

		for col in minimum_col..=maximum_col {
			tile_index.push(self.write_tile(file, container, level, row, col)?);
		}

		let index_start = file.stream_position()?;
		write_compressed_index(file, tile_index)?;
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
			let tile = container.get_tile_uncompressed(level, col, row).unwrap();
			return match self.tile_compression {
				Some(TileCompression::None) => write_vec(file, &tile),
				Some(TileCompression::Gzip) => write_vec_gzip(file, &tile),
				Some(TileCompression::Brotli) => write_vec_brotli(file, &tile),
				None => panic!(),
			};
		} else {
			let tile = container.get_tile_raw(level, col, row).unwrap();
			return write_vec(file, &tile);
		}
	}
}

fn write_vec_brotli(file: &mut BufWriter<File>, data: &Vec<u8>) -> std::io::Result<ByteRange> {
	let params = &BrotliEncoderParams::default();
	let mut cursor = Cursor::new(data);
	let range = ByteRange::new(
		file.stream_position()?,
		BrotliCompress(&mut cursor, file, params)? as u64,
	);
	return Ok(range);
}

fn write_vec_gzip(file: &mut BufWriter<File>, data: &Vec<u8>) -> std::io::Result<ByteRange> {
	let mut buffer: Vec<u8> = Vec::new();
	GzEncoder::new(data.as_slice(), Compression::best()).read_to_end(&mut buffer)?;
	return write_vec(file, &buffer);
}

fn write_vec(file: &mut BufWriter<File>, data: &Vec<u8>) -> std::io::Result<ByteRange> {
	Ok(ByteRange::new(
		file.stream_position()?,
		file.write(&data)? as u64,
	))
}

fn write_compressed_index(
	file: &mut BufWriter<File>,
	index: Vec<ByteRange>,
) -> std::io::Result<ByteRange> {
	let mut buffer: Vec<u8> = Vec::with_capacity(index.len() * 16);
	for range in index {
		range.write_to_vec(&mut buffer)?;
	}
	return write_vec_brotli(file, &buffer);
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
		file.write_at(&self.offset.to_le_bytes(), pos)?;
		file.write_at(&self.length.to_le_bytes(), pos + 8)?;
		return Ok(());
	}
	fn write_to_vec(self, buffer: &mut Vec<u8>) -> std::io::Result<()> {
		buffer.write(&self.offset.to_le_bytes())?;
		buffer.write(&self.length.to_le_bytes())?;
		return Ok(());
	}
}
