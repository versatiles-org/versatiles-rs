mod value;
mod vector_layer;

use crate::{
	types::{Blob, GeoBBox, GeoCenter, TileBBoxPyramid},
	utils::{parse_json_str, JsonObject, JsonValue},
};
use anyhow::{anyhow, ensure, Result};
use regex::Regex;
use std::fmt::Debug;
use value::TileJsonValues;
use vector_layer::VectorLayers;

/// A struct representing a TileJSON object.
///
/// Fields:
/// - `bounds`: An optional geographic bounding box (`[west, south, east, north]`).
/// - `center`: An optional geographic center (`[lon, lat, zoom]`).
/// - `values`: A flexible map of additional TileJSON key-value pairs.
/// - `vector_layers`: A structured set of vector layer definitions.
#[derive(Clone, PartialEq, Default)]
pub struct TileJSON {
	/// Geographic bounding box. If `Some`, `[west, south, east, north]`.
	pub bounds: Option<GeoBBox>,
	/// Geographic center. If `Some`, `[longitude, latitude, zoom_level]`.
	pub center: Option<GeoCenter>,
	/// Other TileJSON fields not explicitly tracked by `TileJSON`.
	pub values: TileJsonValues,
	/// The collection of vector layers, if any.
	pub vector_layers: VectorLayers,
}

impl TileJSON {
	/// Constructs a `TileJSON` from a [`JsonObject`].
	///
	/// Looks for special keys: `"bounds"`, `"center"`, and `"vector_layers"`.
	/// All other keys go into `self.values`.
	///
	/// # Errors
	///
	/// Returns an error if any of these keys are present but invalid
	/// (e.g. cannot be parsed to `GeoBBox`), or if required fields are missing.
	pub fn from_object(object: &JsonObject) -> Result<TileJSON> {
		let mut r = TileJSON::default();
		for (k, v) in object.iter() {
			match k.as_str() {
				"bounds" => {
					// Convert the "bounds" array to a GeoBBox
					let arr = v.as_array()?.as_number_vec()?;
					r.bounds = Some(GeoBBox::try_from(arr)?);
				}
				"center" => {
					// Convert the "center" array to a GeoCenter
					let arr = v.as_array()?.as_number_vec()?;
					r.center = Some(GeoCenter::try_from(arr)?);
				}
				"vector_layers" => {
					// Convert the "vector_layers" array/object into VectorLayers
					r.vector_layers =
						VectorLayers::from_json(v).map_err(|e| anyhow!("Failed to parse 'vector_layers': {e}"))?;
				}
				_ => {
					// Everything else goes into `values`
					r.values.insert(k, v)?;
				}
			}
		}

		Ok(r)
	}

	/// Converts this `TileJSON` into a [`JsonObject`], including all known fields
	/// (`bounds`, `center`, and `vector_layers`) plus the extra fields in `values`.
	pub fn as_object(&self) -> JsonObject {
		let mut obj = JsonObject::default();

		// Insert all `values` first
		for (k, v) in self.values.iter_json_values() {
			obj.set(&k, v);
		}

		// Overwrite with known fields
		obj.set_optional("bounds", &self.bounds.as_ref().map(|b| b.as_vec()));
		obj.set_optional("center", &self.center.as_ref().map(|c| c.as_vec()));
		obj.set_optional("vector_layers", &self.vector_layers.as_json_value_option());
		obj
	}

	/// Converts this `TileJSON` to a pretty-printed JSON string.
	pub fn as_string(&self) -> String {
		self.as_object().stringify()
	}

	/// Converts this `TileJSON` to a `Blob`.
	pub fn as_blob(&self) -> Blob {
		Blob::from(self.as_string())
	}

	/// Updates this `TileJSON` based on a [`TileBBoxPyramid`].
	///
	/// - If `pyramid` includes a `GeoBBox`, calls [`limit_bbox`].
	/// - If `pyramid` includes `zoom_min`, calls [`limit_min_zoom`].
	/// - If `pyramid` includes `zoom_max`, calls [`limit_max_zoom`].
	pub fn update_from_pyramid(&mut self, pyramid: &TileBBoxPyramid) {
		if let Some(bbox) = pyramid.get_geo_bbox() {
			self.limit_bbox(bbox);
		}

		if let Some(z) = pyramid.get_zoom_min() {
			self.limit_min_zoom(z);
		}

		if let Some(z) = pyramid.get_zoom_max() {
			self.limit_max_zoom(z);
		}
	}

	/// Returns a `String` value from `self.values`, if available.
	pub fn get_string(&self, key: &str) -> Option<String> {
		self.values.get_string(key)
	}

	/// Returns a string slice from `self.values`, if available.
	pub fn get_str(&self, key: &str) -> Option<&str> {
		self.values.get_str(key)
	}

	/// Sets a byte (`u8`) value in `self.values`.
	pub fn set_byte(&mut self, key: &str, value: u8) -> Result<()> {
		self.values.insert(key, &JsonValue::from(value))
	}

	/// Sets a list (`Vec<String>`) value in `self.values`.
	pub fn set_list(&mut self, key: &str, value: Vec<String>) -> Result<()> {
		self.values.insert(key, &JsonValue::from(value))
	}

	/// Sets a string value in `self.values`.
	pub fn set_string(&mut self, key: &str, value: &str) -> Result<()> {
		self.values.insert(key, &JsonValue::from(value))
	}

	/// Parses and sets vector layers from a [`JsonValue`].
	///
	/// # Errors
	///
	/// Fails if the `JsonValue` cannot be converted to valid `VectorLayers`.
	pub fn set_vector_layers(&mut self, json: &JsonValue) -> Result<()> {
		self.vector_layers = VectorLayers::from_json(json).map_err(|e| anyhow!("Failed to parse vector layers: {e}"))?;
		Ok(())
	}

	/// Intersects the current bounding box with `bbox`, if one exists; otherwise sets it.
	pub fn limit_bbox(&mut self, bbox: GeoBBox) {
		if let Some(ref mut b) = self.bounds {
			b.intersect(&bbox);
		} else {
			self.bounds = Some(bbox);
		}
	}

	/// Raises the `minzoom` value to `z` if the current `minzoom` is lower (or absent).
	///
	/// Example: if `minzoom` was 3 and `z=5`, then new `minzoom` becomes 5.
	pub fn limit_min_zoom(&mut self, z: u8) {
		self.values.update_byte("minzoom", |mz| mz.map_or(z, |mz| mz.max(z)));
	}

	/// Lowers the `maxzoom` value to `z` if the current `maxzoom` is higher (or absent).
	///
	/// Example: if `maxzoom` was 15 and `z=10`, then new `maxzoom` becomes 10.
	pub fn limit_max_zoom(&mut self, z: u8) {
		self.values.update_byte("maxzoom", |mz| mz.map_or(z, |mz| mz.min(z)));
	}

	/// Merges another `TileJSON` into this one.
	///
	/// - **Bounds:**  
	///   If `other` has a `bounds`, extended or sets ours.
	/// - **Center:**  
	///   Overwrites if `other.center` is `Some`.
	/// - **minzoom/maxzoom:**  
	///   Take the min or max of the two. (By spec, these are `[0..30]`.)
	/// - **Other `values`:**  
	///   Merge everything else, overwriting where conflicts arise.
	/// - **Vector layers:**  
	///   Merges all vector layers from `other`. Overwrites existing layers if IDs match.
	///
	/// # Errors
	///
	/// Returns any error that might arise from inserting the merged values
	/// into `self.values`.
	pub fn merge(&mut self, other: &TileJSON) -> Result<()> {
		// 1. Merge bounds
		if let Some(ob) = &other.bounds {
			self.bounds = match &self.bounds {
				Some(sb) => Some(sb.extended(ob)),
				None => Some(*ob),
			};
		}

		// 2. Overwrite center
		if other.center.is_some() {
			self.center = other.center;
		}

		// 3. Merge minzoom / maxzoom
		if let Some(omiz) = other.values.get_byte("minzoom") {
			let miz = self.values.get_byte("minzoom").map_or(omiz, |mz| mz.min(omiz));
			self.values.insert("minzoom", &JsonValue::from(miz))?;
		}
		if let Some(omaz) = other.values.get_byte("maxzoom") {
			let maz = self.values.get_byte("maxzoom").map_or(omaz, |mz| mz.max(omaz));
			self.values.insert("maxzoom", &JsonValue::from(maz))?;
		}

		// 4. Merge everything else
		for (k, v) in other.values.iter_json_values() {
			if k != "minzoom" && k != "maxzoom" {
				self.values.insert(&k, &v)?;
			}
		}

		// 5. Merge vector_layers
		self.vector_layers.merge(&other.vector_layers)?;

		Ok(())
	}

	/// Converts this `TileJSON` to a JSON string.
	pub fn stringify(&self) -> String {
		self.as_object().stringify()
	}

	/// Performs basic checks (common to both raster and vector) based on the TileJSON 3.0.0 spec.
	///
	/// Ensures that:
	/// - `"tilejson"` exists and matches the pattern `^[123]\.[012]\.[01]$`
	/// - `"tiles"`, `"attribution"`, `"data"`, `"description"`, `"grids"`, `"legend"`,
	///   `"name"`, `"scheme"`, `"template"` and others are optionally valid if present.
	/// - `bounds` and `center` are in valid range if present.
	fn check_basics(&self) -> Result<()> {
		// 3.1 tilejson - required
		let version = self
			.values
			.get_string("tilejson")
			.ok_or_else(|| anyhow!("Missing tilejson"))?;
		ensure!(
			Regex::new(r"^[123]\.[012]\.[01]$")?.is_match(&version),
			"Invalid tilejson version"
		);

		// 3.2 tiles - optional
		self.values.check_optional_list("tiles")?;

		// 3.3 vector_layers - is validated separately in check_vector() or check_raster()

		// 3.4 attribution - optional
		self.values.check_optional_string("attribution")?;

		// 3.5 bounds - optional
		if let Some(b) = self.bounds {
			b.check()?;
		}

		// 3.6 center - optional
		if let Some(c) = self.center {
			c.check()?;
		}

		// 3.7 data - optional
		self.values.check_optional_list("data")?;

		// 3.8 description - optional
		self.values.check_optional_string("description")?;

		// 3.9 fillzoom - optional
		self.values.check_optional_byte("fillzoom")?;

		// 3.10 grids - optional
		self.values.check_optional_list("grids")?;

		// 3.11 legend - optional
		self.values.check_optional_string("legend")?;

		// 3.12 maxzoom - optional
		self.values.check_optional_byte("maxzoom")?;

		// 3.13 minzoom - optional
		self.values.check_optional_byte("minzoom")?;

		// 3.14 name - optional
		self.values.check_optional_string("name")?;

		// 3.15 scheme - optional
		self.values.check_optional_string("scheme")?;

		// 3.16 template - optional
		self.values.check_optional_string("template")?;

		// 3.17 version - optional
		if let Some(v) = self.values.get_string("version") {
			ensure!(Regex::new(r"^\d+\.\d+\.\d+$")?.is_match(&v), "Invalid version number");
		}

		Ok(())
	}

	/// Checks that this `TileJSON` represents a valid **raster** tileset.
	///
	/// - Must pass `check_basics()`.
	/// - Must not have any `vector_layers`.
	pub fn check_raster(&self) -> Result<()> {
		self.check_basics()?;
		ensure!(
			self.vector_layers.0.is_empty(),
			"Raster tilesets must not have 'vector_layers'"
		);
		Ok(())
	}

	/// Checks that this `TileJSON` represents a valid **vector** tileset.
	///
	/// - Must pass `check_basics()`.
	/// - Must have at least one vector layer.
	/// - The layers themselves must pass their checks.
	pub fn check_vector(&self) -> Result<()> {
		self.check_basics()?;
		ensure!(
			!self.vector_layers.0.is_empty(),
			"Vector tilesets must have 'vector_layers'"
		);
		self.vector_layers.check()?;
		Ok(())
	}
}

impl TryFrom<&str> for TileJSON {
	type Error = anyhow::Error;

	/// Parses a JSON string to build a `TileJSON`.
	///
	/// # Errors
	///
	/// Returns an error if the JSON is invalid or doesn't map to a valid `TileJSON`.
	fn try_from(text: &str) -> Result<TileJSON> {
		let object = parse_json_str(text)?.to_object()?;
		TileJSON::from_object(&object)
	}
}

impl TryFrom<&String> for TileJSON {
	type Error = anyhow::Error;

	fn try_from(text: &String) -> Result<TileJSON> {
		TileJSON::try_from(text.as_str())
	}
}

impl TryFrom<&Blob> for TileJSON {
	type Error = anyhow::Error;

	fn try_from(blob: &Blob) -> Result<TileJSON> {
		TileJSON::try_from(blob.as_str())
	}
}

impl From<TileJSON> for String {
	fn from(val: TileJSON) -> Self {
		val.stringify()
	}
}

impl From<TileJSON> for Blob {
	fn from(val: TileJSON) -> Self {
		Blob::from(val.stringify())
	}
}

impl From<&TileJSON> for Blob {
	fn from(val: &TileJSON) -> Self {
		Blob::from(val.stringify())
	}
}

impl Debug for TileJSON {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		// Provide a short debug containing the JSON representation
		write!(f, "TileJSON({})", self.as_string())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Helper function to build a basic TileJSON structure as a JSON object.
	fn make_test_json_object() -> JsonObject {
		let mut obj = JsonObject::default();
		// Must have "tilejson"
		obj.set("tilejson", JsonValue::from("3.0.0"));
		// Minimal valid fields
		obj.set("center", JsonValue::from(vec![100.0, 50.0, 3.0]));
		obj.set("bounds", JsonValue::from(vec![-180.0, -90.0, 180.0, 90.0]));
		obj
	}

	#[test]
	fn test_from_object_basic() -> Result<()> {
		let obj = make_test_json_object();
		let tj = TileJSON::from_object(&obj)?;
		assert!(tj.bounds.is_some());
		assert!(tj.center.is_some());
		assert_eq!(tj.values.get_string("tilejson"), Some("3.0.0".to_string()));
		Ok(())
	}

	#[test]
	fn test_check_raster_ok() -> Result<()> {
		// Raster TileJSON must have tilejson and no vector_layers
		let obj = make_test_json_object();
		let tj = TileJSON::from_object(&obj)?;
		// By default, vector_layers is empty, so check_raster should pass
		assert!(tj.check_raster().is_ok());
		Ok(())
	}

	#[test]
	fn test_check_vector_fails_if_no_layers() -> Result<()> {
		// Vector must have layers -> fails
		let obj = make_test_json_object();
		let tj = TileJSON::from_object(&obj)?;
		assert!(tj.check_vector().is_err());
		Ok(())
	}

	#[test]
	fn test_merge_minmaxzoom() -> Result<()> {
		let mut tj1 = TileJSON::default();
		tj1.set_byte("minzoom", 5)?;
		tj1.set_byte("maxzoom", 15)?;

		let mut tj2 = TileJSON::default();
		tj2.set_byte("minzoom", 2)?;
		tj2.set_byte("maxzoom", 20)?;

		tj1.merge(&tj2)?;
		// minzoom should be min(5,2)=2, maxzoom= max(15,20)=20
		assert_eq!(tj1.values.get_byte("minzoom"), Some(2));
		assert_eq!(tj1.values.get_byte("maxzoom"), Some(20));
		Ok(())
	}

	#[test]
	fn test_limit_bbox() {
		let mut tj = TileJSON::default();
		let existing = GeoBBox(-10.0, -5.0, 10.0, 5.0);
		let newbox = GeoBBox(-15.0, -10.0, 0.0, 2.0);
		tj.bounds = Some(existing);
		tj.limit_bbox(newbox);
		// Intersection of existing and new => [-10, -5, 0, 2]
		let b = tj.bounds.unwrap();
		assert_eq!(b.as_array(), [-10.0, -5.0, 0.0, 2.0]);
	}

	#[test]
	fn test_update_from_pyramid() {
		let mut tj = TileJSON::default();
		// Suppose we have no bounds, so we expect it to be set from the pyramid.
		let bbox_pyramid = TileBBoxPyramid::from_geo_bbox(2, 12, &GeoBBox(-180.0, -90.0, 180.0, 90.0));
		tj.update_from_pyramid(&bbox_pyramid);
		assert_eq!(
			tj.bounds.unwrap().as_array(),
			[-180.0, -85.05112877980659, 180.0, 85.05112877980659]
		);
		assert_eq!(tj.values.get_byte("minzoom"), Some(2));
		assert_eq!(tj.values.get_byte("maxzoom"), Some(12));
	}

	#[test]
	fn test_try_from_str_valid() -> Result<()> {
		let json_text = r#"
        {
            "tilejson": "3.0.0",
            "bounds": [-180, -90, 180, 90],
            "center": [0.0, 0.0, 3.0]
        }
        "#;
		let tj = TileJSON::try_from(json_text)?;
		assert!(tj.bounds.is_some());
		assert!(tj.center.is_some());
		assert_eq!(tj.values.get_string("tilejson"), Some("3.0.0".to_string()));
		Ok(())
	}

	#[test]
	fn test_check_basics_raster_tilejson() {
		let mut obj = JsonObject::default();
		// Provide some other field
		obj.set("bounds", JsonValue::from(vec![0.0, 0.0, 1.0, 1.0]));

		let tj = TileJSON::from_object(&obj).unwrap();
		let result = tj.check_raster();
		assert!(result.is_ok());
	}

	#[test]
	fn test_debug_implementation() {
		let tj = TileJSON::default();
		let debug_str = format!("{:?}", tj);
		// Contains "TileJSON" and the JSON output
		assert!(debug_str.contains("TileJSON("));
		assert!(debug_str.contains("\"tilejson\":\"3.0.0\""));
	}
}
