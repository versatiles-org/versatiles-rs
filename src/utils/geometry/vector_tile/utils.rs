#![allow(dead_code)]

use crate::{types::Blob, utils::BlobWriter};
use anyhow::{Context, Result};
use byteorder::ByteOrder;

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
