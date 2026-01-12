//! This module defines the `GeoCollection` struct, a container for multiple geographic features (`GeoFeature`).
//! It is typically used for representing GeoJSON FeatureCollections.

use super::GeoFeature;
use crate::geojson::parse_geojson;
use anyhow::Result;
use versatiles_core::json::{JsonObject, JsonValue};

/// Represents a collection of `GeoFeature` instances corresponding to a GeoJSON FeatureCollection.
///
/// Provides methods for creation, parsing from GeoJSON strings, and serialization to JSON.
pub struct GeoCollection {
	/// The vector of geographic features contained in this collection.
	pub features: Vec<GeoFeature>,
}

impl GeoCollection {
	/// Constructs a new `GeoCollection` from a vector of `GeoFeature` objects.
	///
	/// # Arguments
	///
	/// * `features` - A vector of `GeoFeature` instances to include in the collection.
	pub fn from(features: Vec<GeoFeature>) -> Self {
		Self { features }
	}

	/// Parses a GeoJSON string and returns a corresponding `GeoCollection`.
	///
	/// # Arguments
	///
	/// * `json_str` - A string slice containing the GeoJSON data to parse.
	///
	/// # Errors
	///
	/// Returns an error if the input string cannot be parsed into a valid `GeoCollection`.
	pub fn from_json_str(json_str: &str) -> Result<Self> {
		parse_geojson(json_str)
	}

	/// Converts the `GeoCollection` into a `JsonObject` compatible with GeoJSON format.
	///
	/// # Arguments
	///
	/// * `precision` - An optional precision value for rounding coordinates.
	///
	/// # Returns
	///
	/// A `JsonObject` representing the GeoJSON FeatureCollection.
	pub fn to_json(&self, precision: Option<u8>) -> JsonObject {
		let mut obj = JsonObject::new();
		obj.set("type", JsonValue::from("FeatureCollection"));
		let features_json = JsonValue::from(self.features.iter().map(|f| f.to_json(precision)).collect::<Vec<_>>());
		obj.set("features", features_json);
		obj
	}
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	use super::*;

	#[test]
	fn test_from_empty() {
		let collection = GeoCollection::from(vec![]);
		assert!(collection.features.is_empty());
	}

	#[test]
	fn test_from_with_features() {
		let feature1 = GeoFeature::new_example();
		let feature2 = GeoFeature::new_example();
		let collection = GeoCollection::from(vec![feature1, feature2]);
		assert_eq!(collection.features.len(), 2);
	}

	#[test]
	fn test_from_json_str_valid() {
		let json = r#"{
			"type": "FeatureCollection",
			"features": [
				{"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]},"properties":{}}
			]
		}"#;
		let collection = GeoCollection::from_json_str(json).unwrap();
		assert_eq!(collection.features.len(), 1);
	}

	#[test]
	fn test_from_json_str_invalid() {
		let json = r#"{"type": "InvalidType", "features": []}"#;
		let result = GeoCollection::from_json_str(json);
		assert!(result.is_err());
	}

	#[test]
	fn test_to_json_empty() {
		let collection = GeoCollection::from(vec![]);
		let json = collection.to_json(None);
		assert_eq!(json.get("type").unwrap().as_str().unwrap(), "FeatureCollection");
		assert!(json.get("features").unwrap().as_array().unwrap().is_empty());
	}

	#[test]
	fn test_to_json_with_features() {
		let feature = GeoFeature::new_example();
		let collection = GeoCollection::from(vec![feature]);
		let json = collection.to_json(None);
		assert_eq!(json.get("type").unwrap().as_str().unwrap(), "FeatureCollection");
		assert_eq!(json.get("features").unwrap().as_array().unwrap().len(), 1);
	}

	#[test]
	fn test_to_json_with_precision() {
		let json_str = r#"{
			"type": "FeatureCollection",
			"features": [
				{"type":"Feature","geometry":{"type":"Point","coordinates":[1.123456789,2.987654321]},"properties":{}}
			]
		}"#;
		let collection = GeoCollection::from_json_str(json_str).unwrap();
		let json = collection.to_json(Some(2));
		let features = json.get("features").unwrap().as_array().unwrap();
		let feature = features.as_vec().first().unwrap();
		let geom = feature
			.as_object()
			.unwrap()
			.get("geometry")
			.unwrap()
			.as_object()
			.unwrap();
		let coords = geom
			.get("coordinates")
			.unwrap()
			.as_array()
			.unwrap()
			.as_number_vec()
			.unwrap();
		assert_eq!(coords[0], 1.12);
		assert_eq!(coords[1], 2.99);
	}
}
