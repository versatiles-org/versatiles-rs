use super::{
	attributes::AttributeLookup, feature::VectorTileFeature, utils::BlobReaderPBF, utils::BlobWriterPBF,
	value::GeoValuePBF,
};
use crate::utils::{
	geometry::types::{Feature, GeoValue},
	BlobReader, BlobWriter,
};
use anyhow::{anyhow, bail, Result};
use byteorder::LE;

#[derive(Debug, Default, PartialEq)]
pub struct VectorTileLayer {
	pub version: u32,
	pub name: String,
	pub features: Vec<Feature>,
	pub extent: u32,
}

impl VectorTileLayer {
	pub fn read(reader: &mut BlobReader<LE>) -> Result<VectorTileLayer> {
		let mut attributes_lookup = AttributeLookup::new();
		let mut vt_features: Vec<VectorTileFeature> = Vec::new();
		let mut version = 1;
		let mut name = None;
		let mut extent = 4096;

		while reader.has_remaining() {
			match reader.read_pbf_key()? {
				(1, 2) => name = Some(reader.read_pbf_string()?),
				(2, 2) => vt_features.push(VectorTileFeature::read(&mut reader.get_pbf_sub_reader()?)?),
				(3, 2) => attributes_lookup.add_key(reader.read_pbf_string()?),
				(4, 2) => attributes_lookup.add_value(GeoValue::read(&mut reader.get_pbf_sub_reader()?)?),
				(5, 0) => extent = reader.read_varint()? as u32,
				(15, 0) => version = reader.read_varint()? as u32,
				(f, w) => bail!("Unexpected combination of field number ({f}) and wire type ({w})"),
			}
		}

		let mut features: Vec<Feature> = Vec::new();
		for vt_feature in vt_features {
			features.push(vt_feature.to_feature(&attributes_lookup)?.into_feature());
		}

		Ok(VectorTileLayer {
			version,
			name: name.ok_or(anyhow!("Layer name is required"))?,
			features,
			extent,
		})
	}

	pub fn write(&self, writer: &mut BlobWriter<LE>) -> Result<()> {
		writer.write_pbf_key(1, 2)?;
		writer.write_pbf_string(&self.name)?;

		todo!("build features");
		let features: Vec<VectorTileFeature> = Vec::new();
		for feature in features {
			writer.write_pbf_key(2, 2)?;
			feature.write(writer)?;
		}

		todo!("build attributes");
		let mut attributes = AttributeLookup::new();
		for key in attributes.keys {
			writer.write_pbf_key(3, 2)?;
			writer.write_pbf_string(&key)?;
		}
		for value in attributes.values {
			writer.write_pbf_key(4, 2)?;
			value.write(writer)?;
		}

		if self.extent != 4096 {
			writer.write_pbf_key(5, 0)?;
			writer.write_varint(self.extent as u64)?;
		}

		if self.version != 1 {
			writer.write_pbf_key(15, 0)?;
			writer.write_varint(self.version as u64)?;
		}

		Ok(())
	}
}
