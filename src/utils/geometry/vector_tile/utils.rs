#![allow(dead_code)]

use crate::{
	types::Blob,
	utils::{BlobReader, BlobWriter},
};
use anyhow::Result;
use byteorder::ByteOrder;

pub trait BlobReaderPBF<'a, E: ByteOrder> {
	fn read_pbf_key(&mut self) -> Result<(u32, u8)>;
	fn read_pbf_packed_uint32(&mut self) -> Result<Vec<u32>>;
	fn read_pbf_string(&mut self) -> Result<String>;
	fn read_pbf_blob(&mut self) -> Result<Blob>;
	fn get_pbf_sub_reader(&mut self) -> Result<BlobReader<E>>;
}

impl<'a, E: ByteOrder> BlobReaderPBF<'a, E> for BlobReader<'a, E> {
	fn read_pbf_key(&mut self) -> Result<(u32, u8)> {
		let value = self.read_varint()?;
		Ok(((value >> 3) as u32, (value & 0x07) as u8))
	}

	fn read_pbf_packed_uint32(&mut self) -> Result<Vec<u32>> {
		let mut reader = self.get_pbf_sub_reader()?;
		let mut values = Vec::new();
		while reader.has_remaining() {
			values.push(reader.read_varint()? as u32);
		}
		Ok(values)
	}

	fn get_pbf_sub_reader(&mut self) -> Result<BlobReader<E>> {
		let length = self.read_varint()?;
		self.get_sub_reader(length)
	}

	fn read_pbf_string(&mut self) -> Result<String> {
		let length = self.read_varint()?;
		self.read_string(length)
	}

	fn read_pbf_blob(&mut self) -> Result<Blob> {
		let length = self.read_varint()?;
		self.read_blob(length)
	}
}

pub trait BlobWriterPBF {
	fn write_pbf_key(&mut self, field_number: u32, wire_type: u8) -> Result<()>;
	fn write_pbf_packed_uint32(&mut self, data: &[u32]) -> Result<()>;
	fn write_pbf_blob(&mut self, blob: &Blob) -> Result<()>;
	fn write_pbf_string(&mut self, text: &str) -> Result<()>;
}

impl<E: ByteOrder> BlobWriterPBF for BlobWriter<E> {
	fn write_pbf_key(&mut self, field_number: u32, wire_type: u8) -> Result<()> {
		self.write_varint(((field_number as u64) << 3) | (wire_type as u64))
	}

	fn write_pbf_packed_uint32(&mut self, data: &[u32]) -> Result<()> {
		let mut writer = BlobWriter::new_le();
		for &value in data {
			writer.write_varint(value as u64)?;
		}
		self.write_pbf_blob(&writer.into_blob())
	}

	fn write_pbf_blob(&mut self, blob: &Blob) -> Result<()> {
		self.write_varint(blob.len())?;
		self.write_blob(&blob)
	}

	fn write_pbf_string(&mut self, text: &str) -> Result<()> {
		self.write_varint(text.len() as u64)?;
		self.write_string(&text)
	}
}
