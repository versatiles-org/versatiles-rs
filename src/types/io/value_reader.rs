#![allow(dead_code)]

use crate::types::{Blob, ByteRange};
use anyhow::{anyhow, bail, Context, Result};
use byteorder::{BigEndian, ByteOrder, LittleEndian, ReadBytesExt};
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::marker::PhantomData;

pub trait SeekRead: Seek + Read {}

pub trait ValueReader<'a, E: ByteOrder + 'a> {
	fn get_reader(&mut self) -> &mut dyn SeekRead;

	fn len(&self) -> u64;
	fn position(&mut self) -> u64;
	fn set_position(&mut self, position: u64) -> Result<()>;

	fn is_empty(&self) -> bool {
		self.len() == 0
	}

	fn remaining(&mut self) -> u64 {
		self.len() - self.position()
	}

	fn has_remaining(&mut self) -> bool {
		self.remaining() > 0
	}

	fn read_varint(&mut self) -> Result<u64> {
		let mut value = 0;
		let mut shift = 0;
		loop {
			let byte = self.get_reader().read_u8()?;
			value |= ((byte as u64) & 0x7F) << shift;
			if byte & 0x80 == 0 {
				break;
			}
			shift += 7;
			if shift >= 70 {
				bail!("Varint too long");
			}
		}
		Ok(value)
	}

	fn read_svarint(&mut self) -> Result<i64> {
		let sint_value = self.read_varint()? as i64;
		Ok((sint_value >> 1) ^ -(sint_value & 1))
	}

	fn read_f32(&mut self) -> Result<f32> {
		Ok(self.get_reader().read_f32::<E>()?)
	}

	fn read_f64(&mut self) -> Result<f64> {
		Ok(self.get_reader().read_f64::<E>()?)
	}

	fn read_u8(&mut self) -> Result<u8> {
		Ok(self.get_reader().read_u8()?)
	}

	fn read_i32(&mut self) -> Result<i32> {
		Ok(self.get_reader().read_i32::<E>()?)
	}

	fn read_i64(&mut self) -> Result<i64> {
		Ok(self.get_reader().read_i64::<E>()?)
	}

	fn read_u32(&mut self) -> Result<u32> {
		Ok(self.get_reader().read_u32::<E>()?)
	}

	fn read_u64(&mut self) -> Result<u64> {
		Ok(self.get_reader().read_u64::<E>()?)
	}

	fn read_blob(&mut self, length: u64) -> Result<Blob> {
		let mut blob = Blob::new_sized(length as usize);
		self.get_reader().read_exact(blob.as_mut_slice())?;
		Ok(blob)
	}

	fn read_string(&mut self, length: u64) -> Result<String> {
		let mut vec = vec![0u8; length as usize];
		self.get_reader().read_exact(&mut vec)?;
		Ok(String::from_utf8(vec)?)
	}

	fn read_range(&mut self) -> Result<ByteRange> {
		Ok(ByteRange::new(
			self.get_reader().read_u64::<E>()?,
			self.get_reader().read_u64::<E>()?,
		))
	}
	fn read_pbf_key(&mut self) -> Result<(u32, u8)> {
		let value = self.read_varint().context("Failed to read varint for PBF key")?;
		Ok(((value >> 3) as u32, (value & 0x07) as u8))
	}

	fn get_sub_reader<'b>(&'b mut self, length: u64) -> Result<Box<dyn ValueReader<'b, E> + 'b>>
	where
		E: 'b;

	fn get_pbf_sub_reader<'b>(&'b mut self) -> Result<Box<dyn ValueReader<'b, E> + 'b>>
	where
		E: 'b,
	{
		let length = self
			.read_varint()
			.context("Failed to read varint for sub-reader length")?;
		self.get_sub_reader(length).context("Failed to get sub-reader")
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
		drop(reader);
		Ok(values)
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
