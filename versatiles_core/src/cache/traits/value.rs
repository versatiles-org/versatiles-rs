use crate::{Blob, TileCoord};
use anyhow::{Result, anyhow};
use byteorder::{LittleEndian as LE, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read};

pub trait CacheValue: Clone {
	fn write_to_cache(&self, writer: &mut Vec<u8>) -> Result<()>;
	fn read_from_cache(reader: &mut Cursor<&[u8]>) -> Result<Self>;
}

impl CacheValue for u8 {
	fn write_to_cache(&self, writer: &mut Vec<u8>) -> Result<()> {
		writer.write_u8(*self)?;
		Ok(())
	}

	fn read_from_cache(reader: &mut Cursor<&[u8]>) -> Result<Self> {
		let value = reader.read_u8()?;
		Ok(value)
	}
}

impl CacheValue for u32 {
	fn write_to_cache(&self, writer: &mut Vec<u8>) -> Result<()> {
		writer.write_u32::<LE>(*self)?;
		Ok(())
	}

	fn read_from_cache(reader: &mut Cursor<&[u8]>) -> Result<Self> {
		let value = reader.read_u32::<LE>()?;
		Ok(value)
	}
}

impl CacheValue for String {
	fn write_to_cache(&self, writer: &mut Vec<u8>) -> Result<()> {
		let bytes = self.as_bytes();
		writer.write_u32::<LE>(bytes.len() as u32)?;
		writer.extend_from_slice(bytes);
		Ok(())
	}

	fn read_from_cache(reader: &mut Cursor<&[u8]>) -> Result<Self> {
		let length = reader.read_u32::<LE>()? as usize;
		let mut bytes = vec![0u8; length];
		reader.read_exact(&mut bytes)?;
		String::from_utf8(bytes).map_err(|e| anyhow!(e))
	}
}

impl<T: CacheValue> CacheValue for Vec<T> {
	fn write_to_cache(&self, writer: &mut Vec<u8>) -> Result<()> {
		writer.write_u32::<LE>(self.len() as u32)?;
		for item in self {
			item.write_to_cache(writer)?;
		}
		Ok(())
	}

	fn read_from_cache(reader: &mut Cursor<&[u8]>) -> Result<Self> {
		let length = reader.read_u32::<LE>()? as usize;
		let mut vec = Vec::with_capacity(length);
		for _ in 0..length {
			vec.push(T::read_from_cache(reader)?);
		}
		Ok(vec)
	}
}

impl<A: CacheValue, B: CacheValue> CacheValue for (A, B) {
	fn write_to_cache(&self, writer: &mut Vec<u8>) -> Result<()> {
		self.0.write_to_cache(writer)?;
		self.1.write_to_cache(writer)
	}

	fn read_from_cache(reader: &mut Cursor<&[u8]>) -> Result<Self> {
		let a = A::read_from_cache(reader)?;
		let b = B::read_from_cache(reader)?;
		Ok((a, b))
	}
}

impl CacheValue for TileCoord {
	fn write_to_cache(&self, writer: &mut Vec<u8>) -> Result<()> {
		writer.write_u8(self.level)?;
		writer.write_u32::<LE>(self.x)?;
		writer.write_u32::<LE>(self.y)?;
		Ok(())
	}

	fn read_from_cache(reader: &mut Cursor<&[u8]>) -> Result<Self> {
		let level = reader.read_u8()?;
		let x = reader.read_u32::<LE>()?;
		let y = reader.read_u32::<LE>()?;
		Ok(TileCoord { level, x, y })
	}
}

impl CacheValue for Blob {
	fn write_to_cache(&self, writer: &mut Vec<u8>) -> Result<()> {
		writer.write_u64::<LE>(self.len())?;
		writer.extend_from_slice(self.as_slice());
		Ok(())
	}

	fn read_from_cache(reader: &mut Cursor<&[u8]>) -> Result<Self> {
		let length = reader.read_u64::<LE>()? as usize;
		let mut bytes = vec![0u8; length];
		reader.read_exact(&mut bytes)?;
		Ok(Blob::from(bytes))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	fn roundtrip<T>(value: T)
	where
		T: CacheValue + PartialEq + core::fmt::Debug,
	{
		let mut buf = vec![];
		value.write_to_cache(&mut buf).unwrap();

		let mut cursor = Cursor::new(buf.as_slice());
		assert_eq!(cursor.position(), 0);

		let decoded = T::read_from_cache(&mut cursor).unwrap();
		assert_eq!(decoded, value);
		assert_eq!(cursor.position(), buf.len() as u64);
	}

	#[rstest]
	#[case(vec![])]
	#[case(vec![0])]
	#[case(vec![0, 1, 2, 3, 4, 5])]
	#[case(vec![255; 1024])] // 1KB of 0xFF bytes
	#[case(vec![0, 255, 128, 10, 200, 0])] // include non-UTF8 bytes to ensure raw bytes are preserved
	fn vec_u8_roundtrips_various_payloads(#[case] payload: Vec<u8>) {
		roundtrip::<Vec<u8>>(payload);
	}

	#[rstest]
	#[case("")] // empty string
	#[case("hello world")] // simple ASCII
	#[case("Grüße 🌍 — こんにちは")] // Unicode
	#[case("naïve café")] // Unicode with accents
	#[case("a".repeat(1000))] // long string
	fn string_roundtrips_ascii_and_unicode(#[case] payload: String) {
		roundtrip::<String>(payload.to_string());
	}

	#[test]
	fn string_from_cache_buffer_panics_on_invalid_utf8() {
		// Construct a buffer that is not valid UTF-8 (single 0xFF byte)
		let invalid = [0xFFu8, 0xFEu8, 0x00u8];
		assert!(String::read_from_cache(&mut Cursor::new(&invalid)).is_err());
	}
}
