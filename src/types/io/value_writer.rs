#![allow(dead_code)]

use crate::types::{Blob, ByteRange};
use anyhow::{Context, Result};
use byteorder::{ByteOrder, WriteBytesExt};
use std::io::Write;

use super::ValueWriterBlob;

pub trait ValueWriter<E: ByteOrder> {
	fn get_writer(&mut self) -> &mut dyn Write;
	fn position(&self) -> u64;

	fn is_empty(&self) -> bool {
		self.position() == 0
	}

	fn write_varint(&mut self, mut value: u64) -> Result<()> {
		while value >= 0x80 {
			self.get_writer().write_all(&[((value as u8) & 0x7F) | 0x80])?;
			value >>= 7;
		}
		self.get_writer().write_all(&[value as u8])?;
		Ok(())
	}

	fn write_svarint(&mut self, value: i64) -> Result<()> {
		self.write_varint(((value << 1) ^ (value >> 63)) as u64)
	}

	fn write_u8(&mut self, value: u8) -> Result<()> {
		Ok(self.get_writer().write_u8(value)?)
	}

	fn write_i32(&mut self, value: i32) -> Result<()> {
		Ok(self.get_writer().write_i32::<E>(value)?)
	}

	fn write_f32(&mut self, value: f32) -> Result<()> {
		Ok(self.get_writer().write_f32::<E>(value)?)
	}

	fn write_f64(&mut self, value: f64) -> Result<()> {
		Ok(self.get_writer().write_f64::<E>(value)?)
	}

	fn write_u32(&mut self, value: u32) -> Result<()> {
		Ok(self.get_writer().write_u32::<E>(value)?)
	}

	fn write_u64(&mut self, value: u64) -> Result<()> {
		Ok(self.get_writer().write_u64::<E>(value)?)
	}

	fn write_blob(&mut self, blob: &Blob) -> Result<()> {
		self.get_writer().write_all(blob.as_slice())?;
		Ok(())
	}

	fn write_slice(&mut self, buf: &[u8]) -> Result<()> {
		self.get_writer().write_all(buf)?;
		Ok(())
	}

	fn write_string(&mut self, text: &str) -> Result<()> {
		self.get_writer().write_all(text.as_bytes())?;
		Ok(())
	}

	fn write_range(&mut self, range: &ByteRange) -> Result<()> {
		self.get_writer().write_u64::<E>(range.offset)?;
		self.get_writer().write_u64::<E>(range.length)?;
		Ok(())
	}

	fn write_pbf_key(&mut self, field_number: u32, wire_type: u8) -> Result<()> {
		self
			.write_varint(((field_number as u64) << 3) | (wire_type as u64))
			.context("Failed to write PBF key")
	}

	fn write_pbf_packed_uint32(&mut self, data: &[u32]) -> Result<()> {
		let mut writer = ValueWriterBlob::new_le();
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
