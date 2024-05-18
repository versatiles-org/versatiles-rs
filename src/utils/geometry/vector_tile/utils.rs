#![allow(dead_code)]

use crate::{
	types::Blob,
	utils::{BlobReader, BlobWriter},
};
use anyhow::{Context, Result};
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
		let value = self.read_varint().context("Failed to read varint for PBF key")?;
		Ok(((value >> 3) as u32, (value & 0x07) as u8))
	}

	fn read_pbf_packed_uint32(&mut self) -> Result<Vec<u32>> {
		let mut reader = self
			.get_pbf_sub_reader()
			.context("Failed to get PBF sub-reader for packed uint32")?;
		let mut values = Vec::new();
		while reader.has_remaining() {
			values.push(
				reader
					.read_varint()
					.context("Failed to read varint for packed uint32")? as u32,
			);
		}
		Ok(values)
	}

	fn get_pbf_sub_reader(&mut self) -> Result<BlobReader<E>> {
		let length = self
			.read_varint()
			.context("Failed to read varint for sub-reader length")?;
		self.get_sub_reader(length).context("Failed to get sub-reader")
	}

	fn read_pbf_string(&mut self) -> Result<String> {
		let length = self.read_varint().context("Failed to read varint for string length")?;
		self.read_string(length).context("Failed to read PBF string")
	}

	fn read_pbf_blob(&mut self) -> Result<Blob> {
		let length = self.read_varint().context("Failed to read varint for blob length")?;
		self.read_blob(length).context("Failed to read PBF blob")
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
		self
			.write_varint(((field_number as u64) << 3) | (wire_type as u64))
			.context("Failed to write PBF key")
	}

	fn write_pbf_packed_uint32(&mut self, data: &[u32]) -> Result<()> {
		let mut writer = BlobWriter::new_le();
		for &value in data {
			writer
				.write_varint(value as u64)
				.context("Failed to write varint for packed uint32")?;
		}
		self
			.write_pbf_blob(&writer.into_blob())
			.context("Failed to write packed uint32 blob")
	}

	fn write_pbf_blob(&mut self, blob: &Blob) -> Result<()> {
		self
			.write_varint(blob.len())
			.context("Failed to write varint for blob length")?;
		self.write_blob(blob).context("Failed to write PBF blob")
	}

	fn write_pbf_string(&mut self, text: &str) -> Result<()> {
		self
			.write_varint(text.len() as u64)
			.context("Failed to write varint for string length")?;
		self.write_string(text).context("Failed to write PBF string")
	}
}
