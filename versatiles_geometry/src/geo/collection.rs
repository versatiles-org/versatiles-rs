use super::GeoFeature;
use crate::geojson::parse_geojson;
use anyhow::Result;
use versatiles_core::json::{JsonObject, JsonValue};

pub struct GeoCollection {
	pub features: Vec<GeoFeature>,
}

impl GeoCollection {
	pub fn from(features: Vec<GeoFeature>) -> Self {
		Self { features }
	}

	pub fn from_json_str(json_str: &str) -> Result<Self> {
		parse_geojson(json_str)
	}

	pub fn to_json(&self, precision: Option<u8>) -> JsonObject {
		let mut obj = JsonObject::new();
		obj.set("type", JsonValue::from("FeatureCollection"));
		let features_json = JsonValue::from(self.features.iter().map(|f| f.to_json(precision)).collect::<Vec<_>>());
		obj.set("features", features_json);
		obj
	}
}
