#![allow(dead_code)]
//! Vector Tile **Layer** utilities.
//!
//! This module defines [`VectorTileLayer`], which represents a single layer in a
//! Mapbox Vector Tile (MVT) / protobuf-encoded tile. It provides reading/writing
//! of the protobuf binary, property table management, and helpers to convert
//! between compact vector-tile features and higher‑level [`GeoFeature`]s.
//!
//! The encoding follows the MVT schema:
//!  * field 1: `name` (string)
//!  * field 2: repeated `feature` (embedded message)
//!  * field 3: repeated `keys` (string)
//!  * field 4: repeated `values` (embedded message)
//!  * field 5: `extent` (varint, default 4096)
//!  * field 15: `version` (varint, default 1)

use crate::{
	geo::{GeoFeature, GeoProperties, GeoValue},
	vector_tile::{feature::VectorTileFeature, property_manager::PropertyManager, value::GeoValuePBF},
};
use anyhow::{Context, Result, anyhow, bail};
use byteorder::LE;
use std::mem::swap;
use versatiles_core::{
	Blob,
	io::{ValueReader, ValueWriter, ValueWriterBlob},
};

/// A single vector‑tile layer with features, key/value property tables, extent, and version.
///
/// The layer stores features in compact vector‑tile form. The `property_manager` maintains
/// the global key and value tables required by the MVT spec; features reference properties
/// by index via `tag_ids`. Helper methods convert to and from high‑level [`GeoFeature`]
/// values for easier processing and GeoJSON export.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorTileLayer {
	/// Tile coordinate extent used to quantize geometry (default 4096).
	pub extent: u32,
	/// The layer's vector‑tile features (geometry + tags).
	pub features: Vec<VectorTileFeature>,
	/// Human‑readable layer name (MVT field 1).
	pub name: String,
	/// Global key/value tables shared by all features in this layer.
	pub property_manager: PropertyManager,
	/// MVT layer version (default 1).
	pub version: u32,
}

impl VectorTileLayer {
	/// Creates a new layer with the given `name`, `extent`, and `version`.
	///
	/// Does not add any features; initializes an empty property table.
	#[must_use]
	pub fn new(name: String, extent: u32, version: u32) -> VectorTileLayer {
		VectorTileLayer {
			extent,
			features: vec![],
			name,
			property_manager: PropertyManager::default(),
			version,
		}
	}

	/// Convenience constructor using the common defaults `extent = 4096`, `version = 1`.
	#[must_use]
	pub fn new_standard(name: &str) -> VectorTileLayer {
		VectorTileLayer::new(name.to_string(), 4096, 1)
	}

	/// Reads a `VectorTileLayer` from a protobuf stream using the MVT wire format.
	///
	/// Expects the fields as defined by the MVT spec and collects keys/values into
	/// the `property_manager`. Returns an error on malformed inputs.
	pub fn read(reader: &mut dyn ValueReader<'_, LE>) -> Result<VectorTileLayer> {
		let mut extent = 4096;
		let mut features: Vec<VectorTileFeature> = Vec::new();
		let mut name = None;
		let mut property_manager = PropertyManager::new();
		let mut version = 1;

		while reader.has_remaining() {
			match reader.read_pbf_key().context("Failed to read PBF key")? {
				(1, 2) => name = Some(reader.read_pbf_string().context("Failed to read layer name")?),
				(2, 2) => features.push(
					VectorTileFeature::read(
						reader
							.get_pbf_sub_reader()
							.context("Failed to get PBF sub-reader for feature")?
							.as_mut(),
					)
					.context("Failed to read VectorTileFeature")?,
				),
				(3, 2) => {
					property_manager.add_key(reader.read_pbf_string().context("Failed to read property key")?);
				}
				(4, 2) => {
					property_manager.add_val(
						GeoValue::read(
							reader
								.get_pbf_sub_reader()
								.context("Failed to get PBF sub-reader for property value")?
								.as_mut(),
						)
						.context("Failed to read GeoValue")?,
					);
				}
				(5, 0) => extent = u32::try_from(reader.read_varint().context("Failed to read extent")?)?,
				(15, 0) => version = u32::try_from(reader.read_varint().context("Failed to read version")?)?,
				(f, w) => bail!("Unexpected combination of field number ({f}) and wire type ({w})"),
			}
		}

		Ok(VectorTileLayer {
			extent,
			features,
			name: name
				.ok_or(anyhow!("Layer name is required"))
				.context("Failed to get layer name")?,
			property_manager,
			version,
		})
	}

	/// Serializes the layer into a protobuf `Blob` (MVT wire format).
	///
	/// Writes name, features, key/value tables, and non‑default `extent`/`version`.
	pub fn to_blob(&self) -> Result<Blob> {
		let mut writer = ValueWriterBlob::new_le();

		writer
			.write_pbf_key(1, 2)
			.context("Failed to write PBF key for layer name")?;
		writer
			.write_pbf_string(&self.name)
			.context("Failed to write layer name")?;

		for feature in &self.features {
			writer
				.write_pbf_key(2, 2)
				.context("Failed to write PBF key for feature")?;
			writer
				.write_pbf_blob(&feature.to_blob().context("Failed to convert feature to blob")?)
				.context("Failed to write feature blob")?;
		}

		for key in self.property_manager.iter_key() {
			writer
				.write_pbf_key(3, 2)
				.context("Failed to write PBF key for property key")?;
			writer.write_pbf_string(key).context("Failed to write property key")?;
		}

		for value in self.property_manager.iter_val() {
			writer
				.write_pbf_key(4, 2)
				.context("Failed to write PBF key for property value")?;
			writer
				.write_pbf_blob(&value.to_blob().context("Failed to convert property value to blob")?)
				.context("Failed to write property value blob")?;
		}

		if self.extent != 4096 {
			writer
				.write_pbf_key(5, 0)
				.context("Failed to write PBF key for extent")?;
			writer
				.write_varint(u64::from(self.extent))
				.context("Failed to write extent")?;
		}

		if self.version != 1 {
			writer
				.write_pbf_key(15, 0)
				.context("Failed to write PBF key for version")?;
			writer
				.write_varint(u64::from(self.version))
				.context("Failed to write version")?;
		}

		Ok(writer.into_blob())
	}

	/// Converts all vector‑tile features into high‑level [`GeoFeature`]s using this layer's property tables.
	pub fn to_features(&self) -> Result<Vec<GeoFeature>> {
		let mut features = Vec::new();
		for feature in &self.features {
			features.push(
				feature
					.to_feature(self)
					.context("Failed to convert VectorTileFeature to MultiFeature")?,
			);
		}
		Ok(features)
	}

	/// Filters/mutates features by decoding their properties, applying a filter that may drop the feature,
	/// recomputing the global property tables, and re‑encoding tag ids. Returns an error if decoding fails.
	pub fn filter_map_properties<F>(&mut self, filter_fn: F) -> Result<()>
	where
		F: Fn(GeoProperties) -> Option<GeoProperties>,
	{
		let mut features: Vec<VectorTileFeature> = vec![];
		swap(&mut features, &mut self.features);

		let feature_prop_list = features
			.into_iter()
			.filter_map(
				|feature: VectorTileFeature| match self.decode_tag_ids(&feature.tag_ids) {
					Ok(p) => filter_fn(p).map(|properties| Ok((feature, properties))),
					Err(_) => None,
				},
			)
			.collect::<Result<Vec<(VectorTileFeature, GeoProperties)>>>()?;

		self.property_manager = PropertyManager::from_iter(feature_prop_list.iter().map(|(_, p)| p));

		self.features = feature_prop_list
			.into_iter()
			.map(|(mut f, p)| {
				f.tag_ids = self.encode_tag_ids(p);
				Ok(f)
			})
			.collect::<Result<Vec<VectorTileFeature>>>()?;

		Ok(())
	}

	/// Transforms properties of all features (non‑dropping). Decodes properties, maps them, rebuilds
	/// the property tables, and re‑encodes tag ids.
	pub fn map_properties<F>(&mut self, filter_fn: F) -> Result<()>
	where
		F: Fn(GeoProperties) -> GeoProperties,
	{
		let mut features: Vec<VectorTileFeature> = vec![];
		swap(&mut features, &mut self.features);

		let feature_prop_list = features
			.into_iter()
			.map(|feature: VectorTileFeature| {
				let properties = filter_fn(self.decode_tag_ids(&feature.tag_ids)?);
				Ok((feature, properties))
			})
			.collect::<Result<Vec<(VectorTileFeature, GeoProperties)>>>()?;

		self.property_manager = PropertyManager::from_iter(feature_prop_list.iter().map(|(_, p)| p));

		self.features = feature_prop_list
			.into_iter()
			.map(|(mut f, p)| {
				f.tag_ids = self.encode_tag_ids(p);
				Ok(f)
			})
			.collect::<Result<Vec<VectorTileFeature>>>()?;

		Ok(())
	}

	/// Adds a `VectorTileFeature` with explicit properties by encoding its tag ids against the current property tables.
	pub fn add_vector_tile_features(&mut self, mut feature: VectorTileFeature, properties: GeoProperties) {
		feature.tag_ids = self.encode_tag_ids(properties);
		self.features.push(feature);
	}

	/// Merges another layer's features into `self`, decoding their properties with the source layer's tables
	/// and re‑encoding them against this layer's `property_manager`.
	pub fn add_from_layer(&mut self, mut layer: VectorTileLayer) -> Result<()> {
		let mut features = vec![];
		swap(&mut features, &mut layer.features);
		for feature in features {
			let properties = layer.decode_tag_ids(&feature.tag_ids)?;
			self.add_vector_tile_features(feature, properties);
		}
		Ok(())
	}

	/// Retains only features that satisfy `filter_fn` (applies to raw `VectorTileFeature`s).
	pub fn retain_features<F>(&mut self, filter_fn: F)
	where
		F: Fn(&VectorTileFeature) -> bool,
	{
		self.features.retain(filter_fn);
	}

	/// Encodes a property map to vector‑tile `tag_ids` using/expanding this layer's property tables.
	pub fn encode_tag_ids(&mut self, properties: GeoProperties) -> Vec<u32> {
		self.property_manager.encode_tag_ids(properties)
	}

	/// Decodes vector‑tile `tag_ids` back into a property map using this layer's property tables.
	pub fn decode_tag_ids(&self, tag_ids: &[u32]) -> Result<GeoProperties> {
		self.property_manager.decode_tag_ids(tag_ids)
	}

	/// Builds a layer from high‑level [`GeoFeature`]s.
	///
	/// Aggregates all properties into key/value tables and converts geometries into `VectorTileFeature`s
	/// with encoded `tag_ids`.
	pub fn from_features(name: String, features: Vec<GeoFeature>, extent: u32, version: u32) -> Result<VectorTileLayer> {
		let mut property_manager = PropertyManager::from_iter(features.iter().map(|f| &f.properties));

		let features = features
			.into_iter()
			.map(|feature| {
				let id = feature.id.map(|id| id.as_u64()).transpose()?;
				VectorTileFeature::from_geometry(
					id,
					property_manager.encode_tag_ids(feature.properties),
					feature.geometry,
				)
			})
			.collect::<Result<Vec<VectorTileFeature>>>()?;

		Ok(VectorTileLayer {
			extent,
			features,
			name,
			property_manager,
			version,
		})
	}

	/// Test helper that constructs a deterministic example layer with one example feature.
	#[cfg(test)]
	pub fn new_example() -> Self {
		VectorTileLayer::from_features(String::from("layer1"), vec![GeoFeature::new_example()], 4096, 1).unwrap()
	}
}

#[cfg(test)]
mod tests {
	use super::super::geometry_type::GeomType;
	use super::*;
	use versatiles_core::io::ValueReaderSlice;

	// ========================================================================
	// Test Helpers
	// ========================================================================

	/// Creates a point feature with the given id and coordinates
	fn point_feature(id: u64, x: f64, y: f64) -> GeoFeature {
		GeoFeature {
			id: Some(GeoValue::from(id)),
			geometry: crate::geo::Geometry::new_point([x, y]),
			properties: GeoProperties::default(),
		}
	}

	/// Creates a point feature with the given id, coordinates, and properties
	fn point_feature_with_props(id: u64, x: f64, y: f64, props: Vec<(&str, GeoValue)>) -> GeoFeature {
		GeoFeature {
			id: Some(GeoValue::from(id)),
			geometry: crate::geo::Geometry::new_point([x, y]),
			properties: GeoProperties::from(props),
		}
	}

	/// Creates a standard test layer from features
	fn make_layer(features: Vec<GeoFeature>) -> Result<VectorTileLayer> {
		VectorTileLayer::from_features("test".to_string(), features, 4096, 1)
	}

	/// Creates a layer with a single example feature
	fn make_example_layer() -> Result<VectorTileLayer> {
		make_layer(vec![GeoFeature::new_example()])
	}

	// ========================================================================
	// Core functionality tests
	// ========================================================================

	#[test]
	fn test_read_vector_tile_layer() -> Result<()> {
		// Example data for a vector tile layer
		let data = vec![
			0x0A, 0x05, b'h', b'e', b'l', b'l', b'o', // name: "hello"
			18, 50, 8, 3, 18, 2, 1, 2, 24, 3, 34, 40, 9, 0, 0, 18, 10, 0, 3, 8, 7, 9, 1, 5, 18, 2, 2, 0, 1, 7, 9, 6, 1,
			26, 6, 0, 0, 8, 5, 0, 7, 9, 2, 5, 26, 0, 4, 2, 0, 0, 3, 7, // feature
			0x1A, 0x03, b'k', b'e', b'y', // property key: "key"
			0x22, 0x04, 0x0A, 0x02, b'v', b'l', // property value: "vl"
		];
		let mut reader = ValueReaderSlice::new_le(&data);
		let layer = VectorTileLayer::read(&mut reader)?;

		assert_eq!(layer.name, "hello");
		assert_eq!(layer.features.len(), 1);
		assert_eq!(
			format!("{:?}", layer.property_manager),
			"PropertyManager { key: [\"key\"], val: [String(\"vl\")] }"
		);
		assert_eq!(layer.extent, 4096);
		assert_eq!(layer.version, 1);
		Ok(())
	}

	#[test]
	fn test_to_blob() -> Result<()> {
		let layer = VectorTileLayer {
			name: "hello".to_string(),
			features: vec![VectorTileFeature::new_example()],
			property_manager: PropertyManager::from_slices(&["key"], &["value"]),
			extent: 4096,
			version: 1,
		};
		let blob = layer.to_blob()?;
		let expected_data = vec![
			0x0A, 0x05, b'h', b'e', b'l', b'l', b'o', // name: "hello"
			18, 50, 8, 3, 18, 2, 1, 2, 24, 3, 34, 40, 9, 0, 0, 18, 10, 0, 3, 8, 7, 9, 1, 5, 18, 2, 2, 0, 1, 7, 9, 6, 1,
			26, 6, 0, 0, 8, 5, 0, 7, 9, 2, 5, 26, 0, 4, 2, 0, 0, 3, 7, // feature
			0x1A, 0x03, b'k', b'e', b'y', // property key: "key"
			0x22, 0x07, 0x0A, 0x05, b'v', b'a', b'l', b'u', b'e', // property value: "value"
		];
		assert_eq!(blob.into_vec(), expected_data);
		Ok(())
	}

	#[test]
	fn test_decode_tag_ids() -> Result<()> {
		let mut layer = VectorTileLayer::new("hello".to_string(), 4096, 1);
		layer.property_manager = PropertyManager::from_slices(&["key"], &["value"]);
		assert_eq!(
			layer.decode_tag_ids(&[0, 0])?,
			GeoProperties::from(vec![("key", GeoValue::from("value"))])
		);
		Ok(())
	}

	#[test]
	fn test_to_features() -> Result<()> {
		let feature = GeoFeature::new_example();
		let layer = VectorTileLayer::from_features("hello".to_string(), vec![feature.clone()], 2048, 3)?;
		let features = layer.to_features()?;
		println!("{:?}", features[0].properties);
		assert_eq!(features.len(), 1);
		assert_eq!(
			format!("{:?}", features[0].properties),
			"{\"is_nice\": Bool(true), \"name\": String(\"Nice\"), \"population\": UInt(348085)}"
		);
		Ok(())
	}

	#[test]
	fn test_from_features() -> Result<()> {
		let features = vec![GeoFeature::new_example()];
		let layer = VectorTileLayer::from_features("hello".to_string(), features, 4096, 1)?;
		assert_eq!(layer.name, "hello");
		assert_eq!(layer.features.len(), 1);
		assert_eq!(layer.property_manager.key.list, vec!["is_nice", "name", "population"]);
		assert_eq!(
			layer.property_manager.val.list,
			vec![GeoValue::from("Nice"), GeoValue::from(348085), GeoValue::from(true)]
		);
		assert_eq!(layer.extent, 4096);
		assert_eq!(layer.version, 1);
		Ok(())
	}

	// ========================================================================
	// Tests for map_properties
	// ========================================================================

	#[test]
	fn test_map_properties_add_property() -> Result<()> {
		let mut layer = make_example_layer()?;

		layer.map_properties(|mut props| {
			props.insert("added".to_string(), GeoValue::from("new_value"));
			props
		})?;

		let decoded_props = layer.decode_tag_ids(&layer.features[0].tag_ids)?;
		assert_eq!(decoded_props.get("added"), Some(&GeoValue::from("new_value")));
		assert_eq!(decoded_props.get("name"), Some(&GeoValue::from("Nice")));
		Ok(())
	}

	#[test]
	fn test_map_properties_modify_property() -> Result<()> {
		let mut layer = make_example_layer()?;

		layer.map_properties(|mut props| {
			if let Some(GeoValue::String(s)) = props.get("name") {
				props.insert("name".to_string(), GeoValue::from(format!("{s}_modified")));
			}
			props
		})?;

		let decoded_props = layer.decode_tag_ids(&layer.features[0].tag_ids)?;
		assert_eq!(decoded_props.get("name"), Some(&GeoValue::from("Nice_modified")));
		Ok(())
	}

	#[test]
	fn test_map_properties_remove_property() -> Result<()> {
		let mut layer = make_example_layer()?;

		layer.map_properties(|mut props| {
			props.remove("population");
			props
		})?;

		let decoded_props = layer.decode_tag_ids(&layer.features[0].tag_ids)?;
		assert!(decoded_props.get("population").is_none());
		assert_eq!(decoded_props.get("name"), Some(&GeoValue::from("Nice")));
		assert_eq!(decoded_props.get("is_nice"), Some(&GeoValue::from(true)));
		Ok(())
	}

	#[test]
	fn test_map_properties_multiple_features() -> Result<()> {
		let mut layer = make_layer(vec![
			point_feature_with_props(
				1,
				0.0,
				0.0,
				vec![("name", GeoValue::from("A")), ("value", GeoValue::from(10))],
			),
			point_feature_with_props(
				2,
				1.0,
				1.0,
				vec![("name", GeoValue::from("B")), ("value", GeoValue::from(20))],
			),
		])?;

		layer.map_properties(|mut props| {
			if let Some(GeoValue::UInt(v)) = props.get("value") {
				props.insert("value".to_string(), GeoValue::from(v * 2));
			}
			props
		})?;

		assert_eq!(layer.features.len(), 2);
		let props1 = layer.decode_tag_ids(&layer.features[0].tag_ids)?;
		let props2 = layer.decode_tag_ids(&layer.features[1].tag_ids)?;
		assert_eq!(props1.get("value"), Some(&GeoValue::from(20)));
		assert_eq!(props2.get("value"), Some(&GeoValue::from(40)));
		Ok(())
	}

	#[test]
	fn test_map_properties_rebuilds_property_manager() -> Result<()> {
		let mut layer = make_example_layer()?;

		assert!(layer.property_manager.key.list.contains(&"name".to_string()));
		assert!(layer.property_manager.key.list.contains(&"population".to_string()));

		layer.map_properties(|props| {
			let mut new_props = GeoProperties::default();
			if let Some(name) = props.get("name") {
				new_props.insert("name".to_string(), name.clone());
			}
			new_props
		})?;

		assert_eq!(layer.property_manager.key.list, vec!["name".to_string()]);
		Ok(())
	}

	// ========================================================================
	// Tests for retain_features
	// ========================================================================

	#[test]
	fn test_retain_features_keep_all() -> Result<()> {
		let mut layer = make_layer(vec![point_feature(1, 0.0, 0.0), point_feature(2, 1.0, 1.0)])?;

		layer.retain_features(|_| true);

		assert_eq!(layer.features.len(), 2);
		Ok(())
	}

	#[test]
	fn test_retain_features_remove_all() -> Result<()> {
		let mut layer = make_layer(vec![point_feature(1, 0.0, 0.0), point_feature(2, 1.0, 1.0)])?;

		layer.retain_features(|_| false);

		assert_eq!(layer.features.len(), 0);
		Ok(())
	}

	#[test]
	fn test_retain_features_filter_by_id() -> Result<()> {
		let mut layer = make_layer(vec![
			point_feature(1, 0.0, 0.0),
			point_feature(2, 1.0, 1.0),
			point_feature(3, 2.0, 2.0),
		])?;

		layer.retain_features(|f| f.id.is_some_and(|id| id >= 2));

		assert_eq!(layer.features.len(), 2);
		assert_eq!(layer.features[0].id, Some(2));
		assert_eq!(layer.features[1].id, Some(3));
		Ok(())
	}

	#[test]
	fn test_retain_features_filter_by_geometry_type() -> Result<()> {
		let line_feature = GeoFeature {
			id: Some(GeoValue::from(2)),
			geometry: crate::geo::Geometry::new_line_string(vec![[0.0, 0.0], [1.0, 1.0]]),
			properties: GeoProperties::default(),
		};
		let mut layer = make_layer(vec![point_feature(1, 0.0, 0.0), line_feature])?;

		layer.retain_features(|f| f.geom_type == GeomType::MultiPoint);

		assert_eq!(layer.features.len(), 1);
		assert_eq!(layer.features[0].id, Some(1));
		Ok(())
	}

	// ========================================================================
	// Tests for filter_map_properties
	// ========================================================================

	#[test]
	fn test_filter_map_properties_keep_all() -> Result<()> {
		let mut layer = make_example_layer()?;

		layer.filter_map_properties(Some)?;

		assert_eq!(layer.features.len(), 1);
		Ok(())
	}

	#[test]
	fn test_filter_map_properties_remove_all() -> Result<()> {
		let mut layer = make_example_layer()?;

		layer.filter_map_properties(|_| None)?;

		assert_eq!(layer.features.len(), 0);
		Ok(())
	}

	#[test]
	fn test_filter_map_properties_filter_by_property() -> Result<()> {
		let mut layer = make_layer(vec![
			point_feature_with_props(1, 0.0, 0.0, vec![("keep", GeoValue::from(true))]),
			point_feature_with_props(2, 1.0, 1.0, vec![("keep", GeoValue::from(false))]),
			point_feature_with_props(3, 2.0, 2.0, vec![("keep", GeoValue::from(true))]),
		])?;

		layer.filter_map_properties(|props| {
			if props.get("keep") == Some(&GeoValue::from(true)) {
				Some(props)
			} else {
				None
			}
		})?;

		assert_eq!(layer.features.len(), 2);
		assert_eq!(layer.features[0].id, Some(1));
		assert_eq!(layer.features[1].id, Some(3));
		Ok(())
	}

	#[test]
	fn test_filter_map_properties_filter_and_transform() -> Result<()> {
		let mut layer = make_layer(vec![
			point_feature_with_props(1, 0.0, 0.0, vec![("value", GeoValue::from(10))]),
			point_feature_with_props(2, 1.0, 1.0, vec![("value", GeoValue::from(5))]),
			point_feature_with_props(3, 2.0, 2.0, vec![("value", GeoValue::from(15))]),
		])?;

		layer.filter_map_properties(|mut props| {
			if let Some(GeoValue::UInt(v)) = props.get("value")
				&& *v >= 10
			{
				props.insert("value".to_string(), GeoValue::from(v * 2));
				return Some(props);
			}
			None
		})?;

		assert_eq!(layer.features.len(), 2);
		let props1 = layer.decode_tag_ids(&layer.features[0].tag_ids)?;
		let props2 = layer.decode_tag_ids(&layer.features[1].tag_ids)?;
		assert_eq!(props1.get("value"), Some(&GeoValue::from(20))); // 10 * 2
		assert_eq!(props2.get("value"), Some(&GeoValue::from(30))); // 15 * 2
		Ok(())
	}

	#[test]
	fn test_filter_map_properties_rebuilds_property_manager() -> Result<()> {
		let mut layer = make_layer(vec![
			point_feature_with_props(
				1,
				0.0,
				0.0,
				vec![
					("common", GeoValue::from("shared")),
					("only_in_first", GeoValue::from("value1")),
				],
			),
			point_feature_with_props(
				2,
				1.0,
				1.0,
				vec![
					("common", GeoValue::from("shared")),
					("only_in_second", GeoValue::from("value2")),
				],
			),
		])?;

		layer.filter_map_properties(|props| {
			if props.get("only_in_first").is_some() {
				None
			} else {
				Some(props)
			}
		})?;

		// Property manager should not contain "only_in_first" anymore
		assert!(!layer.property_manager.key.list.contains(&"only_in_first".to_string()));
		assert!(layer.property_manager.key.list.contains(&"common".to_string()));
		assert!(layer.property_manager.key.list.contains(&"only_in_second".to_string()));
		Ok(())
	}

	#[test]
	fn test_filter_map_properties_empty_layer() -> Result<()> {
		let mut layer = VectorTileLayer::new_standard("test");

		// Should handle empty layer gracefully
		layer.filter_map_properties(Some)?;

		assert_eq!(layer.features.len(), 0);
		Ok(())
	}
}
