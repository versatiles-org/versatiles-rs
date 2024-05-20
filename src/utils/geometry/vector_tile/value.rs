#![allow(dead_code)]

use super::utils::BlobWriterPBF;
use crate::{
	types::{Blob, ValueReader},
	utils::{geometry::basic::GeoValue, BlobWriter},
};
use anyhow::{anyhow, bail, Context, Result};
use byteorder::LE;

pub trait GeoValuePBF<'a> {
	fn read(reader: &mut dyn ValueReader<'a, LE>) -> Result<GeoValue>;
	fn to_blob(&self) -> Result<Blob>;
}

impl<'a> GeoValuePBF<'a> for GeoValue {
	fn read(reader: &mut dyn ValueReader<'a, LE>) -> Result<GeoValue> {
		// source: https://protobuf.dev/programming-guides/encoding/

		use GeoValue::*;
		let mut value: Option<GeoValue> = None;

		while reader.has_remaining() {
			value = Some(match reader.read_pbf_key().context("Failed to read PBF key")? {
				// https://protobuf.dev/programming-guides/encoding/#structure
				(1, 2) => {
					let len = reader
						.read_varint()
						.context("Failed to read varint for string length")?;
					String(reader.read_string(len).context("Failed to read string value")?)
				}
				(2, 5) => Float(reader.read_f32().context("Failed to read f32 value")?),
				(3, 1) => Double(reader.read_f64().context("Failed to read f64 value")?),
				(4, 0) => Int(reader.read_varint().context("Failed to read varint for int value")? as i64),
				(5, 0) => UInt(reader.read_varint().context("Failed to read varint for uint value")?),
				(6, 0) => Int(reader.read_svarint().context("Failed to read svarint value")?),
				(7, 0) => Bool(reader.read_varint().context("Failed to read varint for bool value")? != 0),
				(f, w) => bail!("Unexpected combination of field number ({f}) and wire type ({w})"),
			})
		}
		value
			.ok_or_else(|| anyhow!("No value found"))
			.context("Failed to read GeoValue")
	}

	fn to_blob(&self) -> Result<Blob> {
		let mut writer = BlobWriter::new_le();

		match self {
			GeoValue::String(s) => {
				writer
					.write_pbf_key(1, 2)
					.context("Failed to write PBF key for string value")?;
				writer.write_pbf_string(s).context("Failed to write string value")?;
			}
			GeoValue::Float(f) => {
				writer
					.write_pbf_key(2, 5)
					.context("Failed to write PBF key for float value")?;
				writer.write_f32(*f).context("Failed to write float value")?;
			}
			GeoValue::Double(f) => {
				writer
					.write_pbf_key(3, 1)
					.context("Failed to write PBF key for double value")?;
				writer.write_f64(*f).context("Failed to write double value")?;
			}
			GeoValue::UInt(u) => {
				writer
					.write_pbf_key(5, 0)
					.context("Failed to write PBF key for uint value")?;
				writer.write_varint(*u).context("Failed to write uint value")?;
			}
			GeoValue::Int(s) => {
				writer
					.write_pbf_key(6, 0)
					.context("Failed to write PBF key for int value")?;
				writer.write_svarint(*s).context("Failed to write int value")?;
			}
			GeoValue::Bool(b) => {
				writer
					.write_pbf_key(7, 0)
					.context("Failed to write PBF key for bool value")?;
				writer
					.write_varint(if *b { 1 } else { 0 })
					.context("Failed to write bool value")?;
			}
		}

		Ok(writer.into_blob())
	}
}
