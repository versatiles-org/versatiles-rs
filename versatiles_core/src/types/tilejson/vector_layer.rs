use crate::{json::*, types::TileContent};
use anyhow::{Context, Result, anyhow, ensure};
use std::{collections::BTreeMap, fmt::Debug};

/// A collection of [`VectorLayer`]s keyed by their `id` string.
///
/// Corresponds to the "`vector_layers`" array in the `TileJSON` specification:
/// <https://github.com/mapbox/tilejson-spec/tree/master/3.0.0#33-vector_layers>
#[derive(Clone, Default, Debug, PartialEq)]
pub struct VectorLayers(pub BTreeMap<String, VectorLayer>);

impl VectorLayers {
	/// Constructs a [`VectorLayers`] instance from a [`JsonArray`].
	///
	/// # JSON Structure
	///
	/// Each element in the array is expected to be a JSON object with the following keys:
	/// - `"id"`: Required `string`. The identifier for the vector layer.
	/// - `"description"`: Optional `string`. A description of the layer.
	/// - `"minzoom"`: Optional `number` in the range `[0..30]`. Minimum zoom level.
	/// - `"maxzoom"`: Optional `number` in the range `[0..30]`. Maximum zoom level.
	/// - `"fields"`: Required `object`, each key is a field name, and its value is a `string`.
	///
	/// # Errors
	///
	/// Fails if:
	/// - The `"id"` key is missing or invalid.
	/// - The `"fields"` key is missing or invalid.
	/// - The associated values fail to convert to the expected types (`string`, `number`).
	pub fn from_json(json: &JsonValue) -> Result<Self> {
		let array = json
			.as_array()
			.with_context(|| anyhow!("expected 'vector_layers' is an array"))?;

		let mut map = BTreeMap::new();
		for entry in &array.0 {
			// Convert each entry to an object
			let object = entry.as_object()?;

			// Required: "id"
			let id = object.get_string("id")?.ok_or_else(|| anyhow!("missing `id`"))?;

			// Optional: "description", "minzoom", "maxzoom"
			let description = object.get_string("description")?;
			let minzoom = object.get_number("minzoom")?.map(|v| v as u8);
			let maxzoom = object.get_number("maxzoom")?.map(|v| v as u8);

			// Required: "fields" object
			let mut fields = BTreeMap::<String, String>::new();
			if let Some(value) = object.get("fields") {
				for (k, v) in value.as_object()?.iter() {
					fields.insert(k.clone(), v.as_string()?);
				}
			}

			// Build the [`VectorLayer`] and insert into the map
			let layer = VectorLayer {
				fields,
				description,
				minzoom,
				maxzoom,
			};
			map.insert(id, layer);
		}

		Ok(VectorLayers(map))
	}

	/// Converts this collection of layers to an [`Option<JsonValue>`].
	///
	/// Returns `None` if the collection is empty, or `Some(JsonValue::Array(...))`
	/// otherwise.
	#[must_use]
	pub fn as_json_value_option(&self) -> Option<JsonValue> {
		if self.0.is_empty() {
			None
		} else {
			Some(self.as_json_value())
		}
	}

	/// Converts this collection to a [`JsonValue::Array`], where each array element
	/// is a layer represented as a [`JsonObject`].
	///
	/// Each object contains:
	/// - `"id"` (string),
	/// - `"fields"`, `"description"`, `"minzoom"`, `"maxzoom"` if present in the layer
	#[must_use]
	pub fn as_json_value(&self) -> JsonValue {
		JsonValue::from(
			self
				.0
				.iter()
				.map(|(id, layer)| {
					// Construct the object from the layer
					let mut obj = layer.as_json_object();
					// Insert "id"
					obj.set("id", JsonValue::from(id));
					JsonValue::Object(obj)
				})
				.collect::<Vec<JsonValue>>(),
		)
	}

	#[must_use]
	pub fn contains_ids(&self, ids: &[&str]) -> bool {
		ids.iter().all(|id| self.0.contains_key(*id))
	}

	#[must_use]
	pub fn get_tile_schema(&self) -> TileContent {
		if self.contains_ids(&[
			"aerodrome_label",
			"aeroway",
			"boundary",
			"building",
			"housenumber",
			"landcover",
			"landuse",
			"mountain_peak",
			"park",
			"place",
			"poi",
			"transportation",
			"transportation_name",
			"water",
			"water_name",
			"waterway",
		]) {
			TileContent::VectorOpenMapTiles
		} else if self.contains_ids(&[
			"addresses",
			"aerialways",
			"boundaries",
			"boundary_labels",
			"bridges",
			"buildings",
			"dam_lines",
			"dam_polygons",
			"ferries",
			"land",
			"ocean",
			"pier_lines",
			"pier_polygons",
			"place_labels",
			"pois",
			"public_transport",
			"sites",
			"street_labels_points",
			"street_labels",
			"street_polygons",
			"streets_polygons_labels",
			"streets",
			"water_lines_labels",
			"water_lines",
			"water_polygons_labels",
			"water_polygons",
		]) {
			TileContent::VectorShortbread1
		} else {
			TileContent::VectorOther
		}
	}

	/// Checks that all layers conform to the `TileJSON` 3.0.0 spec:
	///
	/// - `id` is non-empty, <= 255 chars, alphanumeric.
	/// - The layer itself passes [`VectorLayer::check`].
	///
	/// # Errors
	///
	/// Returns an error if any constraints are violated.
	pub fn check(&self) -> Result<()> {
		// See: https://github.com/mapbox/tilejson-spec/tree/master/3.0.0#33-vector_layers
		for (id, layer) in &self.0 {
			// 3.3.1 id - required
			ensure!(!id.is_empty(), "Empty layer id");
			ensure!(id.len() <= 255, "Layer id too long: '{id}'");
			ensure!(
				id.chars().all(|c| c.is_ascii_alphanumeric()),
				"Invalid layer id '{id}': must be alphanumeric"
			);

			layer.check().with_context(|| format!("layer '{id}'"))?;
		}
		Ok(())
	}

	/// Merges all layers from `other` into this collection.
	///
	/// - If a layer `id` does not exist in `self`, it is inserted outright.
	/// - If a layer `id` already exists, their contents are merged via [`VectorLayer::merge`].
	///
	/// # Errors
	///
	/// Currently does not fail. The `Result<()>` return type allows
	/// expansion if you want to validate merges or handle conflicts.
	pub fn merge(&mut self, other: &VectorLayers) -> Result<()> {
		for (id, layer) in &other.0 {
			// If the layer already exists, merge the fields
			if let Some(existing) = self.0.get_mut(id) {
				existing.merge(layer);
			} else {
				// Otherwise, insert the whole layer
				self.0.insert(id.clone(), layer.clone());
			}
		}
		Ok(())
	}

	/// Returns a vector of all layer ids in this collection.
	#[must_use]
	pub fn layer_ids(&self) -> Vec<String> {
		self.0.keys().cloned().collect()
	}

	/// Finds a layer by its id.
	/// Returns `None` if the layer does not exist.
	#[must_use]
	pub fn find(&self, id: &str) -> Option<&VectorLayer> {
		self.0.get(id)
	}

	pub fn iter_mut(&mut self) -> std::collections::btree_map::IterMut<'_, String, VectorLayer> {
		self.0.iter_mut()
	}

	pub fn iter(&self) -> std::collections::btree_map::Iter<'_, String, VectorLayer> {
		self.0.iter()
	}
}

impl FromIterator<(String, VectorLayer)> for VectorLayers {
	/// Constructs a [`VectorLayers`] from an iterator of tuples `(String, VectorLayer)`.
	///
	/// This is useful for creating a `VectorLayers` instance from a collection of layers.
	fn from_iter<I: IntoIterator<Item = (String, VectorLayer)>>(iter: I) -> Self {
		VectorLayers(iter.into_iter().collect())
	}
}

/// Represents a single layer entry within "`vector_layers`" in the `TileJSON` spec.
///
/// Each layer has:
/// - `fields`: A mapping from field names -> field types (both `String`).
/// - `description`: An optional textual description of the layer.
/// - `minzoom`, `maxzoom`: Optional `u8` values (0..=30).
#[derive(Clone, Debug, PartialEq)]
pub struct VectorLayer {
	pub fields: BTreeMap<String, String>,
	pub description: Option<String>,
	pub minzoom: Option<u8>,
	pub maxzoom: Option<u8>,
}

impl VectorLayer {
	/// Converts this [`VectorLayer`] into a [`JsonObject`].
	///
	/// The object will include:
	/// - `"fields"` (object)
	/// - `"description"` (string, if present)
	/// - `"minzoom"` (number, if present)
	/// - `"maxzoom"` (number, if present)
	#[must_use]
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

	/// Validates the layer according to the `TileJSON` 3.0.0 spec:
	///
	/// - 3.3.2 fields: required; each key is non-empty, <= 255 chars, alphanumeric
	/// - 3.3.3 description: optional
	/// - 3.3.4 minzoom, maxzoom: optional; must be <= 30, and `minzoom <= maxzoom`
	///
	/// # Errors
	///
	/// Returns an error if any constraints fail.
	pub fn check(&self) -> Result<()> {
		// See: https://github.com/mapbox/tilejson-spec/tree/master/3.0.0#33-vector_layers

		// 3.3.2 fields - required
		for key in self.fields.keys() {
			ensure!(!key.is_empty(), "Empty field name");
			ensure!(key.len() <= 255, "Field name too long: '{key}'");
			ensure!(
				key.chars().all(|c| c.is_ascii_alphanumeric()),
				"Invalid field name '{key}': must be alphanumeric"
			);
		}

		// 3.3.3 description - optional, no explicit constraints.

		// 3.3.4 minzoom, maxzoom - optional, must be <= 30
		if let Some(mz) = self.minzoom {
			ensure!(mz <= 30, "minzoom too high: {mz}");
		}
		if let Some(mz) = self.maxzoom {
			ensure!(mz <= 30, "maxzoom too high: {mz}");
			if let Some(minz) = self.minzoom {
				ensure!(minz <= mz, "minzoom must be <= maxzoom, found min={minz}, max={mz}");
			}
		}
		Ok(())
	}

	/// Merges the fields from `other` into this layer, overwriting existing data where conflicts arise.
	///
	/// - `fields`: All fields from `other` are inserted (overwriting any existing).
	/// - `description`: Overwrites if `other` has one.
	/// - `minzoom`: Takes the smaller of `self`'s and `other`'s (if both exist), else whichever is present.
	/// - `maxzoom`: Takes the larger of `self`'s and `other`'s (if both exist), else whichever is present.
	pub fn merge(&mut self, other: &VectorLayer) {
		// Merge fields
		for (key, value) in &other.fields {
			self.fields.insert(key.clone(), value.clone());
		}

		// Overwrite description if present
		if let Some(desc) = &other.description {
			self.description = Some(desc.clone());
		}

		// Merge minzoom
		if let Some(other_min) = other.minzoom {
			self.minzoom = Some(match self.minzoom {
				Some(m) => m.min(other_min),
				None => other_min,
			});
		}

		// Merge maxzoom
		if let Some(other_max) = other.maxzoom {
			self.maxzoom = Some(match self.maxzoom {
				Some(m) => m.max(other_max),
				None => other_max,
			});
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_from_json_array_basic() -> Result<()> {
		// Create a JSON array with one valid layer object.
		// The layer must have "id" and "fields".
		let json = JsonValue::from(vec![vec![
			("id", JsonValue::from("myLayer")),
			("fields", JsonValue::from(vec![("name", "String")])),
		]]);
		let vector_layers = VectorLayers::from_json(&json)?;
		assert_eq!(vector_layers.0.len(), 1);
		assert!(vector_layers.0.contains_key("myLayer"));
		Ok(())
	}

	#[test]
	fn test_from_json_array_missing_id() {
		// "id" is required, so missing "id" should fail.
		let json = JsonValue::from(vec![vec![("fields", vec![("name", "String")])]]);
		let result = VectorLayers::from_json(&json);
		assert_eq!(result.unwrap_err().to_string(), "missing `id`");
	}

	#[test]
	fn test_from_json_array_missing_fields() {
		// "fields" is required, so missing "fields" should fail.
		let json = JsonValue::from(vec![vec![("id", JsonValue::from("layer1"))]]);
		let result = VectorLayers::from_json(&json).unwrap();
		assert_eq!(
			result.as_json_value().stringify(),
			"[{\"fields\":{},\"id\":\"layer1\"}]"
		);
	}

	#[test]
	fn test_check_valid() -> Result<()> {
		let mut map = BTreeMap::new();
		map.insert(
			"myLayer".to_owned(),
			VectorLayer {
				fields: BTreeMap::from([("field1".to_owned(), "String".to_owned())]),
				description: Some("A simple layer".to_owned()),
				minzoom: Some(0),
				maxzoom: Some(10),
			},
		);
		let vl = VectorLayers(map);
		assert!(vl.check().is_ok());
		Ok(())
	}

	#[test]
	fn test_check_invalid_id() {
		// Non-alphanumeric ID should fail check()
		let mut map = BTreeMap::new();
		map.insert(
			"my.layer!".to_owned(),
			VectorLayer {
				fields: BTreeMap::new(),
				description: None,
				minzoom: None,
				maxzoom: None,
			},
		);
		let vl = VectorLayers(map);
		assert_eq!(
			vl.check().unwrap_err().to_string(),
			"Invalid layer id 'my.layer!': must be alphanumeric"
		);
	}

	#[test]
	fn test_layer_merge() {
		let mut layer1 = VectorLayer {
			fields: BTreeMap::from([
				("name".to_owned(), "String".to_owned()),
				("count".to_owned(), "Integer".to_owned()),
			]),
			description: Some("Layer 1 description".to_owned()),
			minzoom: Some(5),
			maxzoom: Some(10),
		};

		let layer2 = VectorLayer {
			fields: BTreeMap::from([
				("name".to_owned(), "String".to_owned()),
				("type".to_owned(), "String".to_owned()),
			]),
			description: Some("Layer 2 override".to_owned()),
			minzoom: Some(3),
			maxzoom: Some(15),
		};

		layer1.merge(&layer2);
		// Expect "count" and "type" to both exist, "description" to be overwritten
		// and minzoom = min(5,3) = 3, maxzoom = max(10,15) = 15

		assert_eq!(layer1.fields["count"], "Integer");
		assert_eq!(layer1.fields["type"], "String");
		assert_eq!(layer1.description.as_deref(), Some("Layer 2 override"));
		assert_eq!(layer1.minzoom, Some(3));
		assert_eq!(layer1.maxzoom, Some(15));
	}

	#[test]
	fn test_vector_layers_merge() -> Result<()> {
		let mut vl1 = VectorLayers(BTreeMap::from([
			(
				"layerA".to_owned(),
				VectorLayer {
					fields: BTreeMap::from([("fieldA".to_string(), "String".to_string())]),
					description: Some("First layer".to_string()),
					minzoom: Some(2),
					maxzoom: Some(6),
				},
			),
			(
				"layerB".to_owned(),
				VectorLayer {
					fields: BTreeMap::from([("fieldB".to_string(), "String".to_string())]),
					description: Some("Second layer".to_string()),
					minzoom: Some(4),
					maxzoom: Some(8),
				},
			),
		]));

		let vl2 = VectorLayers(BTreeMap::from([
			(
				"layerB".to_owned(),
				VectorLayer {
					fields: BTreeMap::from([
						("fieldB".to_string(), "String".to_string()),
						("fieldC".to_string(), "Integer".to_string()),
					]),
					description: Some("Overridden second".to_string()),
					minzoom: Some(1),
					maxzoom: Some(9),
				},
			),
			(
				"layerC".to_owned(),
				VectorLayer {
					fields: BTreeMap::from([("fieldD".to_string(), "String".to_string())]),
					description: None,
					minzoom: None,
					maxzoom: None,
				},
			),
		]));

		vl1.merge(&vl2)?;

		// Expect that "layerA" is untouched
		assert!(vl1.0.contains_key("layerA"));
		// Expect that "layerB" is merged
		assert!(vl1.0.contains_key("layerB"));
		// Expect that "layerC" is newly inserted
		assert!(vl1.0.contains_key("layerC"));

		let layer_b_merged = vl1.0.get("layerB").unwrap();
		// "fieldB" remains, "fieldC" added, new description, zoom min=1, max=9
		assert!(layer_b_merged.fields.contains_key("fieldB"));
		assert!(layer_b_merged.fields.contains_key("fieldC"));
		assert_eq!(layer_b_merged.description.as_deref(), Some("Overridden second"));
		assert_eq!(layer_b_merged.minzoom, Some(1));
		assert_eq!(layer_b_merged.maxzoom, Some(9));

		Ok(())
	}

	#[test]
	fn test_as_json_value_option() -> Result<()> {
		let mut layers_map = BTreeMap::new();
		let layer = VectorLayer {
			fields: BTreeMap::from([("key".to_string(), "String".to_string())]),
			description: Some("A layer".to_owned()),
			minzoom: Some(0),
			maxzoom: Some(5),
		};
		layers_map.insert("myLayer".to_owned(), layer);
		let layers = VectorLayers(layers_map);

		let json_val_option = layers.as_json_value_option();
		assert!(json_val_option.is_some());

		let json_val = json_val_option.unwrap();
		// Should be an array of length 1
		match &json_val {
			JsonValue::Array(arr) => {
				assert_eq!(arr.0.len(), 1);
				if let JsonValue::Object(obj) = &arr.0[0] {
					// Expect 'id' == 'myLayer'
					let id = obj.get("id").ok_or_else(|| anyhow!("missing 'id'"))?;
					assert_eq!(id.as_string()?, "myLayer");
				} else {
					panic!("Expected a JsonObject in the array.");
				}
			}
			_ => panic!("Expected a JsonValue::Array."),
		}
		Ok(())
	}

	#[test]
	fn test_as_json_value_empty() {
		let empty_layers = VectorLayers::default();
		assert!(empty_layers.as_json_value_option().is_none());
	}

	#[test]
	fn test_contains_ids_and_layer_ids_find_iter() -> Result<()> {
		// Prepare two layers under different keys
		let layer1 = VectorLayer {
			fields: BTreeMap::new(),
			description: None,
			minzoom: None,
			maxzoom: None,
		};
		let layer2 = VectorLayer {
			fields: BTreeMap::new(),
			description: Some("desc".to_string()),
			minzoom: Some(1),
			maxzoom: Some(2),
		};
		let mut map = BTreeMap::new();
		map.insert("b".to_string(), layer2.clone());
		map.insert("a".to_string(), layer1.clone());
		let vl = VectorLayers(map);
		// contains_ids
		assert!(vl.contains_ids(&["a"]));
		assert!(vl.contains_ids(&["a", "b"]));
		assert!(!vl.contains_ids(&["c"]));
		// layer_ids (should be sorted by key)
		assert_eq!(vl.layer_ids(), vec!["a".to_string(), "b".to_string()]);
		// find
		assert_eq!(vl.find("a"), Some(&layer1));
		assert!(vl.find("c").is_none());
		// iter order
		let keys: Vec<String> = vl.iter().map(|(k, _)| k.clone()).collect();
		assert_eq!(keys, vec!["a".to_string(), "b".to_string()]);
		Ok(())
	}

	#[test]
	fn test_get_tile_schema_empty() {
		use TileContent::*;
		let empty = VectorLayers(BTreeMap::new());
		assert_eq!(empty.get_tile_schema(), VectorOther);
	}

	#[test]
	fn test_get_tile_schema_openmaptiles() {
		use TileContent::*;
		let known_open = [
			"aerodrome_label",
			"aeroway",
			"boundary",
			"building",
			"housenumber",
			"landcover",
			"landuse",
			"mountain_peak",
			"park",
			"place",
			"poi",
			"transportation",
			"transportation_name",
			"water",
			"water_name",
			"waterway",
		];
		let mut map = BTreeMap::new();
		for id in known_open {
			map.insert(
				id.to_string(),
				VectorLayer {
					fields: BTreeMap::new(),
					description: None,
					minzoom: None,
					maxzoom: None,
				},
			);
		}
		let vl = VectorLayers(map);
		assert_eq!(vl.get_tile_schema(), VectorOpenMapTiles);
	}

	#[test]
	fn test_get_tile_schema_shortbread1() {
		use TileContent::*;
		let known_sb = [
			"addresses",
			"aerialways",
			"boundaries",
			"boundary_labels",
			"bridges",
			"buildings",
			"dam_lines",
			"dam_polygons",
			"ferries",
			"land",
			"ocean",
			"pier_lines",
			"pier_polygons",
			"place_labels",
			"pois",
			"public_transport",
			"sites",
			"street_labels_points",
			"street_labels",
			"street_polygons",
			"streets_polygons_labels",
			"streets",
			"water_lines_labels",
			"water_lines",
			"water_polygons_labels",
			"water_polygons",
		];
		let mut map = BTreeMap::new();
		for id in known_sb {
			map.insert(
				id.to_string(),
				VectorLayer {
					fields: BTreeMap::new(),
					description: None,
					minzoom: None,
					maxzoom: None,
				},
			);
		}
		let vl = VectorLayers(map);
		assert_eq!(vl.get_tile_schema(), VectorShortbread1);
	}

	#[test]
	fn test_vector_layer_as_json_object_and_check() -> Result<()> {
		let layer = VectorLayer {
			fields: BTreeMap::from([("key".to_string(), "String".to_string())]),
			description: Some("desc".to_string()),
			minzoom: Some(5),
			maxzoom: Some(10),
		};
		let obj = layer.as_json_object();
		// Check object entries
		assert_eq!(obj.get_string("description")?.unwrap(), "desc");
		assert_eq!(obj.get_number("minzoom")?.unwrap(), 5.0);
		assert_eq!(obj.get_number("maxzoom")?.unwrap(), 10.0);
		let fields = obj.get("fields").unwrap().as_object()?;
		assert_eq!(fields.get_string("key")?.unwrap(), "String");
		// check valid layer
		layer.check()?;
		// check invalid minzoom > maxzoom
		let bad = VectorLayer {
			fields: BTreeMap::new(),
			description: None,
			minzoom: Some(3),
			maxzoom: Some(2),
		};
		assert!(
			bad.check()
				.unwrap_err()
				.to_string()
				.contains("minzoom must be <= maxzoom")
		);
		Ok(())
	}
}
