#![allow(dead_code)]

use super::utils::BlobWriterPBF;
use crate::{
	types::Blob,
	utils::{
		geometry::{types::GeoValue, vector_tile::utils::BlobReaderPBF},
		BlobReader, BlobWriter,
	},
};
use anyhow::{anyhow, bail, Result};
use byteorder::LE;

pub trait GeoValuePBF {
	fn read(reader: &mut BlobReader<LE>) -> Result<GeoValue>;
	fn to_blob(&self) -> Result<Blob>;
}

impl GeoValuePBF for GeoValue {
	fn read(reader: &mut BlobReader<LE>) -> Result<GeoValue> {
		// source: https://protobuf.dev/programming-guides/encoding/

		use GeoValue::*;
		let mut value: Option<GeoValue> = None;

		while reader.has_remaining() {
			value = Some(match reader.read_pbf_key()? {
				// https://protobuf.dev/programming-guides/encoding/#structure
				(1, 2) => {
					let len = reader.read_varint()?;
					String(reader.read_string(len)?)
				}
				(2, 5) => Float(reader.read_f32()?),
				(3, 1) => Double(reader.read_f64()?),
				(4, 0) => Int(reader.read_varint()? as i64),
				(5, 0) => UInt(reader.read_varint()?),
				(6, 0) => Int(reader.read_svarint()?),
				(7, 0) => Bool(reader.read_varint()? != 0),
				(f, w) => bail!("Unexpected combination of field number ({f}) and wire type ({w})"),
			})
		}
		value.ok_or(anyhow!("no value found"))
	}

	fn to_blob(&self) -> Result<Blob> {
		let mut writer = BlobWriter::new_le();

		match self {
			GeoValue::String(s) => {
				writer.write_pbf_key(1, 2)?;
				writer.write_pbf_string(s)?;
			}
			GeoValue::Float(f) => {
				writer.write_pbf_key(2, 5)?;
				writer.write_f32(*f)?;
			}
			GeoValue::Double(f) => {
				writer.write_pbf_key(3, 1)?;
				writer.write_f64(*f)?;
			}
			GeoValue::UInt(u) => {
				writer.write_pbf_key(5, 0)?;
				writer.write_varint(*u)?;
			}
			GeoValue::Int(s) => {
				writer.write_pbf_key(6, 0)?;
				writer.write_svarint(*s)?;
			}
			GeoValue::Bool(b) => {
				writer.write_pbf_key(7, 0)?;
				writer.write_varint(if *b { 1 } else { 0 })?;
			}
		}

		Ok(writer.into_blob())
	}
}
