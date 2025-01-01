use crate::utils::{JsonArray, JsonObject, JsonValue};
use anyhow::{anyhow, ensure, Context, Result};
use std::{collections::BTreeMap, fmt::Debug};

/// A collection of [VectorLayer], keyed by `id`.
///
/// Corresponds to the "vector_layers" array in the TileJSON specification.
/// https://github.com/mapbox/tilejson-spec/tree/master/3.0.0#33-vector_layers
#[derive(Clone, Default, Debug, PartialEq)]
pub struct VectorLayers(pub BTreeMap<String, VectorLayer>);

impl VectorLayers {
	/// Creates a [VectorLayers] from a [JsonArray].
	///
	/// Expects each array element to be an object with keys:
	/// - `"id"` (string, required)
	/// - `"description"` (string, optional)
	/// - `"minzoom"` (number, optional)
	/// - `"maxzoom"` (number, optional)
	/// - `"fields"` (object, required)  
	///
	/// Returns an error if any required field is missing or of an invalid type.
	pub fn from_json_array(array: &JsonArray) -> Result<VectorLayers> {
		let mut map = BTreeMap::new();
		for entry in array.0.iter() {
			// Convert each entry to an object
			let object = entry.as_object()?;

			// Required: "id"
			let id = object.get_string("id")?.ok_or_else(|| anyhow!("missing `id`"))?;

			// Optional: "description", "minzoom", "maxzoom"
			let description = object.get_string("description")?;
			let minzoom = object.get_number("minzoom")?;
			let maxzoom = object.get_number("maxzoom")?;

			// Required: "fields", which is an object
			let fields_val = object.get("fields").ok_or(anyhow!("missing `fields`"))?.as_object()?;

			// Convert each entry in "fields" to a (String, String) pair
			let fields = fields_val
				.iter()
				.map(|(k, v)| v.as_string().map(|field_val| (k.clone(), field_val)))
				.collect::<Result<Vec<(String, String)>>>()?;

			// Build the [VectorLayer] and insert into the map
			let layer = VectorLayer {
				description,
				minzoom,
				maxzoom,
				fields: fields.into_iter().collect(),
			};
			map.insert(id, layer);
		}
		Ok(VectorLayers(map))
	}

	/// Converts this collection to an [Option<JsonValue>].
	///
	/// Returns [None] if the map is empty.
	pub fn as_json_value_option(&self) -> Option<JsonValue> {
		if self.0.is_empty() {
			None
		} else {
			Some(self.as_json_value())
		}
	}

	/// Converts this collection to a [JsonValue] (an array of objects).
	///
	/// Each object has:
	/// - `"id"`: `String`  
	/// - `"fields"`, `"description"`, `"minzoom"`, `"maxzoom"` if present in the layer
	pub fn as_json_value(&self) -> JsonValue {
		JsonValue::from(
			self
				.0
				.iter()
				.map(|(key, value)| {
					// Construct a JsonObject from the layer
					let mut obj = value.as_json_object();
					obj.set("id", JsonValue::from(key));
					JsonValue::Object(obj)
				})
				.collect::<Vec<JsonValue>>(),
		)
	}

	/// Checks that all layers follow the TileJSON spec requirements:
	/// 1. The `id` is not empty, no longer than 255 chars, and alphanumeric.
	/// 2. Each layer also passes its own [VectorLayer::check] validation.
	///
	/// Returns an error if any checks fail.
	pub fn check(&self) -> Result<()> {
		// https://github.com/mapbox/tilejson-spec/tree/master/3.0.0#33-vector_layers
		for (id, layer) in &self.0 {
			// 3.3.1 id - required
			ensure!(!id.is_empty(), "Empty key");
			ensure!(id.len() <= 255, "Key too long");
			ensure!(
				id.chars().all(|c| c.is_ascii_alphanumeric()),
				"Invalid key: must be alphanumeric"
			);

			layer.check().with_context(|| format!("layer '{id}'"))?;
		}
		Ok(())
	}

	/// Merges all layers from `other` into this collection.
	/// If a layer `id` already exists, the fields will be merged or overwritten.
	pub fn merge(&mut self, other: &VectorLayers) -> Result<()> {
		for (id, layer) in other.0.iter() {
			if self.0.contains_key(id) {
				// If the layer already exists, merge the fields
				self.0.get_mut(id).unwrap().merge(layer);
			} else {
				// Otherwise, insert the layer
				self.0.insert(id.to_owned(), layer.clone());
			}
		}
		Ok(())
	}
}

/// Represents a single entry in "vector_layers" from TileJSON.
///
/// Fields:
/// - `fields`: A mapping of field name -> field type
/// - `description`: Optional layer description
/// - `minzoom`, `maxzoom`: Optional zoom bounds
#[derive(Clone, Debug, PartialEq)]
pub struct VectorLayer {
	pub fields: BTreeMap<String, String>,
	pub description: Option<String>,
	pub minzoom: Option<u8>,
	pub maxzoom: Option<u8>,
}

impl VectorLayer {
	/// Converts this [VectorLayer] into a [JsonObject].
	///
	/// Output object includes:
	/// - `"fields"` (object)
	/// - `"description"` (string, if present)
	/// - `"minzoom"` (number, if present)
	/// - `"maxzoom"` (number, if present)
	pub fn as_json_object(&self) -> JsonObject {
		let mut obj = JsonObject::default();

		// Convert the 'fields' map to a JSON object
		obj.set(
			"fields",
			JsonValue::from(
				self
					.fields
					.iter()
					.map(|(key, val)| (key.as_str(), JsonValue::from(val)))
					.collect::<Vec<(&str, JsonValue)>>(),
			),
		);

		// Optionally include other fields if they're present
		if let Some(desc) = &self.description {
			obj.set("description", JsonValue::from(desc));
		}
		if let Some(minz) = self.minzoom {
			obj.set("minzoom", JsonValue::from(minz));
		}
		if let Some(maxz) = self.maxzoom {
			obj.set("maxzoom", JsonValue::from(maxz));
		}

		obj
	}

	/// Performs checks that ensure the layer follows the TileJSON spec.
	///
	/// - 3.3.2 fields - required; each key must be non-empty, <= 255 chars, and alphanumeric
	/// - 3.3.3 description - optional
	/// - 3.3.4 minzoom, maxzoom - optional; must be <= 30, and minzoom <= maxzoom if both set
	///
	/// Returns an error if any checks fail.
	pub fn check(&self) -> Result<()> {
		// https://github.com/mapbox/tilejson-spec/tree/master/3.0.0#33-vector_layers

		// 3.3.2 fields - required
		for key in self.fields.keys() {
			ensure!(!key.is_empty(), "Empty key in 'fields'");
			ensure!(key.len() <= 255, "Key in 'fields' too long");
			ensure!(
				key.chars().all(|c| c.is_ascii_alphanumeric()),
				"Invalid key in 'fields': must be alphanumeric"
			);
		}

		// 3.3.3 description - optional, no explicit constraints in the spec

		// 3.3.4 minzoom, maxzoom - optional, must be <= 30
		if let Some(v0) = self.minzoom {
			ensure!(v0 <= 30, "minzoom too high");
		}
		if let Some(v1) = self.maxzoom {
			ensure!(v1 <= 30, "maxzoom too high");

			if let Some(v0) = self.minzoom {
				ensure!(v0 <= v1, "minzoom must be less than or equal to maxzoom");
			}
		}

		Ok(())
	}

	/// Merges the fields from `other` into this layer.
	/// If a field already exists, it will be overwritten.
	pub fn merge(&mut self, other: &VectorLayer) {
		for (key, value) in &other.fields {
			self.fields.insert(key.to_owned(), value.to_owned());
		}

		if other.description.is_some() {
			self.description = other.description.clone();
		}

		if let Some(minzoom) = other.minzoom {
			self.minzoom = Some(self.minzoom.map_or(minzoom, |mz| mz.min(minzoom)));
		}

		if let Some(maxzoom) = other.maxzoom {
			self.maxzoom = Some(self.maxzoom.map_or(maxzoom, |mz| mz.max(maxzoom)));
		}
	}
}
