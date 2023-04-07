use super::{ByteRange, VersaTilesSrcTrait};
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};
use versatiles_shared::{Blob, Compression, TileFormat};

const HEADER_LENGTH: usize = 66;
const BBOX_SCALE: i32 = 10000000;

#[derive(Debug, PartialEq)]
pub struct FileHeader {
	pub zoom_range: [u8; 2],
	pub bbox: [i32; 4],

	pub tile_format: TileFormat,
	pub compression: Compression,

	pub meta_range: ByteRange,
	pub blocks_range: ByteRange,
}
impl FileHeader {
	pub fn new(tile_format: &TileFormat, compression: &Compression, zoom_range: [u8; 2], bbox: [f32; 4]) -> FileHeader {
		assert!(
			zoom_range[0] <= zoom_range[1],
			"zoom_range[0] ({}) must be <= zoom_range[1] ({})",
			zoom_range[0],
			zoom_range[1]
		);
		assert!(bbox[0] >= -180.0, "bbox[0] ({}) >= -180", bbox[0]);
		assert!(bbox[1] >= -90.0, "bbox[1] ({}) >= -90", bbox[1]);
		assert!(bbox[2] <= 180.0, "bbox[2] ({}) <= 180", bbox[2]);
		assert!(bbox[3] <= 90.0, "bbox[3] ({}) <= 90", bbox[3]);
		assert!(bbox[0] <= bbox[2], "bbox[0] ({}) <= bbox[2] ({})", bbox[0], bbox[2]);
		assert!(bbox[1] <= bbox[3], "bbox[1] ({}) <= bbox[3] ({})", bbox[1], bbox[3]);

		FileHeader {
			zoom_range,
			bbox: bbox.map(|v| (v * BBOX_SCALE as f32) as i32),
			tile_format: tile_format.clone(),
			compression: compression.to_owned(),
			meta_range: ByteRange::empty(),
			blocks_range: ByteRange::empty(),
		}
	}

	pub async fn from_reader(reader: &mut Box<dyn VersaTilesSrcTrait>) -> FileHeader {
		FileHeader::from_blob(
			reader
				.read_range(&ByteRange::new(0, HEADER_LENGTH as u64))
				.await
				.unwrap(),
		)
	}

	pub fn to_blob(&self) -> Blob {
		let mut header: Vec<u8> = Vec::new();
		header.write_all(b"versatiles_v02").unwrap();

		// tile type
		header
			.write_u8(match self.tile_format {
				TileFormat::BIN => 0x00,

				TileFormat::PNG => 0x10,
				TileFormat::JPG => 0x11,
				TileFormat::WEBP => 0x12,
				TileFormat::AVIF => 0x13,
				TileFormat::SVG => 0x14,

				TileFormat::PBF => 0x20,
				TileFormat::GEOJSON => 0x21,
				TileFormat::TOPOJSON => 0x22,
				TileFormat::JSON => 0x23,
			})
			.unwrap();

		// compression
		header
			.write_u8(match self.compression {
				Compression::None => 0,
				Compression::Gzip => 1,
				Compression::Brotli => 2,
			})
			.unwrap();

		header.write_u8(self.zoom_range[0]).unwrap();
		header.write_u8(self.zoom_range[1]).unwrap();

		header.write_i32::<BE>(self.bbox[0]).unwrap();
		header.write_i32::<BE>(self.bbox[1]).unwrap();
		header.write_i32::<BE>(self.bbox[2]).unwrap();
		header.write_i32::<BE>(self.bbox[3]).unwrap();

		self.meta_range.write_to_buf(&mut header);
		self.blocks_range.write_to_buf(&mut header);

		if header.len() != HEADER_LENGTH {
			panic!(
				"header should be {} bytes long, but is {} bytes long",
				HEADER_LENGTH,
				header.len()
			)
		}

		Blob::from(header)
	}

	fn from_blob(blob: Blob) -> FileHeader {
		if blob.len() != HEADER_LENGTH {
			panic!();
		}

		let mut header = Cursor::new(blob.as_slice());
		let mut magic_word = [0u8; 14];
		header.read_exact(&mut magic_word).unwrap();
		if &magic_word != b"versatiles_v02" {
			panic!()
		};

		let tile_type = header.read_u8().unwrap();
		let compression = header.read_u8().unwrap();

		let tile_format = match tile_type {
			0x00 => TileFormat::BIN,

			0x10 => TileFormat::PNG,
			0x11 => TileFormat::JPG,
			0x12 => TileFormat::WEBP,
			0x13 => TileFormat::AVIF,
			0x14 => TileFormat::SVG,

			0x20 => TileFormat::PBF,
			0x21 => TileFormat::GEOJSON,
			0x22 => TileFormat::TOPOJSON,
			0x23 => TileFormat::JSON,
			_ => panic!(),
		};

		let compression = match compression {
			0 => Compression::None,
			1 => Compression::Gzip,
			2 => Compression::Brotli,
			_ => panic!(),
		};

		let zoom_range: [u8; 2] = [header.read_u8().unwrap(), header.read_u8().unwrap()];

		let bbox: [i32; 4] = [
			header.read_i32::<BE>().unwrap(),
			header.read_i32::<BE>().unwrap(),
			header.read_i32::<BE>().unwrap(),
			header.read_i32::<BE>().unwrap(),
		];

		let meta_range = ByteRange::from_reader(&mut header);
		let blocks_range = ByteRange::from_reader(&mut header);

		FileHeader {
			zoom_range,
			bbox,
			tile_format,
			compression,
			meta_range,
			blocks_range,
		}
	}
}

#[cfg(test)]
mod tests {
	use byteorder::ByteOrder;

	use super::*;

	#[test]
	fn conversion() {
		let test = |tile_format: &TileFormat, compression: &Compression, a: u64, b: u64, c: u64, d: u64| {
			let mut header1 = FileHeader::new(tile_format, compression, [0, 0], [0.0, 0.0, 0.0, 0.0]);
			header1.meta_range = ByteRange::new(a, b);
			header1.blocks_range = ByteRange::new(c, d);

			let header2 = FileHeader::from_blob(header1.to_blob());
			assert_eq!(header1, header2);
			assert_eq!(&header2.tile_format, tile_format);
			assert_eq!(&header2.compression, compression);
			assert_eq!(header2.meta_range, ByteRange::new(a, b));
			assert_eq!(header2.blocks_range, ByteRange::new(c, d));
		};
		test(
			&TileFormat::JPG,
			&Compression::None,
			314159265358979323,
			846264338327950288,
			419716939937510582,
			097494459230781640,
		);

		test(&TileFormat::PBF, &Compression::Brotli, 29, 97, 92, 458);
	}

	#[test]
	fn test_new_file_header() {
		let tf = TileFormat::PNG;
		let comp = Compression::Gzip;
		let zoom = [10, 14];
		let bbox = [-180.0, -85.0511, 180.0, 85.0511];
		let header = FileHeader::new(&tf, &comp, zoom, bbox);

		assert_eq!(header.zoom_range, zoom);
		assert_eq!(header.bbox, [-1800000000, -850511040, 1800000000, 850511040]);
		assert_eq!(header.tile_format, tf);
		assert_eq!(header.compression, comp);
		assert_eq!(header.meta_range, ByteRange::empty());
		assert_eq!(header.blocks_range, ByteRange::empty());
	}

	#[test]
	fn test_to_blob() {
		let header = FileHeader::new(
			&TileFormat::PBF,
			&Compression::Gzip,
			[3, 8],
			[-180.0, -85.05112878, 180.0, 85.05112878],
		);

		let blob = header.to_blob();

		assert_eq!(blob.len(), HEADER_LENGTH);
		assert_eq!(&blob.as_slice()[0..14], b"versatiles_v02");
		assert_eq!(blob.as_slice()[14], 0x20);
		assert_eq!(blob.as_slice()[15], 1);
		assert_eq!(blob.as_slice()[16], 3);
		assert_eq!(blob.as_slice()[17], 8);
		assert_eq!(BE::read_i32(&blob.as_slice()[18..22]), -1800000000);
		assert_eq!(BE::read_i32(&blob.as_slice()[22..26]), -850511296);
		assert_eq!(BE::read_i32(&blob.as_slice()[26..30]), 1800000000);
		assert_eq!(BE::read_i32(&blob.as_slice()[30..34]), 850511296);
		assert_eq!(ByteRange::from_buf(&blob.as_slice()[34..50]), ByteRange::empty());
		assert_eq!(ByteRange::from_buf(&blob.as_slice()[50..66]), ByteRange::empty());

		let header2 = FileHeader::from_blob(blob);

		assert_eq!(header2.zoom_range, [3, 8]);
		assert_eq!(header2.bbox, [-1800000000, -850511296, 1800000000, 850511296]);
		assert_eq!(header2.tile_format, TileFormat::PBF);
		assert_eq!(header2.compression, Compression::Gzip);
		assert_eq!(header2.meta_range, ByteRange::empty());
		assert_eq!(header2.blocks_range, ByteRange::empty());
	}
}
