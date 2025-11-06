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
