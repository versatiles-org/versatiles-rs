//! This module defines the `TileJSON` struct, representing a TileJSON object and its fields.
//!
//! A TileJSON can contain:
//! - An optional geographic bounding box, `[west, south, east, north]`.
//! - An optional geographic center, `[longitude, latitude, zoom_level]`.
//! - Additional TileJSON key-value pairs in [`TileJsonValues`].
//! - A collection of vector layers defined in [`VectorLayers`].
//!
//! Methods are provided to parse from JSON, merge with other `TileJSON` objects,
//! and validate according to the TileJSON 3.0.0 specification.
//!
//! # Example
//! ```rust
//! # use versatiles_core::tilejson::*;
//! # async fn example() -> Result<(), anyhow::Error> {
//! let json_text = r#"
//!   {
//!     "tilejson": "3.0.0",
//!     "bounds": [-180, -90, 180, 90],
//!     "center": [0.0, 0.0, 3.0]
//!   }
//! "#;
//!
//! // Parse from JSON string
//! let tilejson = TileJSON::try_from(json_text)?;
//!
//! // Convert back to JSON string or Blob
//! let json_string = tilejson.as_string();
//! let json_blob = tilejson.as_blob();
//! # Ok(())
//! # }
//! ```

mod value;
pub mod vector_layer;

use crate::{json::*, types::*};
use anyhow::{anyhow, ensure, Ok, Result};
use regex::Regex;
use std::fmt::Debug;
use value::TileJsonValues;
use vector_layer::VectorLayers;

/// A struct representing a TileJSON object.
///
/// # Fields
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
	/// Additional key-value pairs not explicitly tracked by this struct.
	pub values: TileJsonValues,
	/// The collection of vector layers, if any.
	pub vector_layers: VectorLayers,
	pub tile_content: Option<TileType>,
	pub tile_format: Option<TileFormat>,
	pub tile_schema: Option<TileSchema>,
}

impl TileJSON {
	// -------------------------------------------------------------------------
	// Creation and Parsing
	// -------------------------------------------------------------------------

	/// Constructs a `TileJSON` by reading from a [`JsonObject`].
	///
	/// Special keys recognized:
	/// - `"bounds"`: Interpreted as a [`GeoBBox`].
	/// - `"center"`: Interpreted as a [`GeoCenter`].
	/// - `"vector_layers"`: Interpreted as [`VectorLayers`].
	/// - Any other key is stored in `self.values`.
	///
	/// # Errors
	/// Returns an error if:
	/// - A known key is present but invalid (e.g., non-numeric bounds).
	/// - Vector layers fail to parse.
	pub fn from_object(object: &JsonObject) -> Result<TileJSON> {
		let mut r = TileJSON::default();
		for (k, v) in object.iter() {
			match k.as_str() {
				"bounds" => {
					// Parse `[west, south, east, north]`
					r.bounds = Some(GeoBBox::try_from(v.as_array()?.as_number_vec()?)?);
				}
				"center" => {
					// Parse `[lon, lat, zoom]`
					r.center = Some(GeoCenter::try_from(v.as_array()?.as_number_vec()?)?);
				}
				"vector_layers" => {
					r.vector_layers =
						VectorLayers::from_json(v).map_err(|e| anyhow!("Failed to parse 'vector_layers': {e}"))?;
				}
				"tile_content" => {
					r.tile_content = Some(TileType::try_from(v.as_str()?)?);
				}
				"tile_format" => {
					r.tile_format = Some(TileFormat::try_from_mime(v.as_str()?)?);
				}
				"tile_schema" => {
					r.tile_schema = Some(TileSchema::try_from(v.as_str()?)?);
				}
				_ => {
					// Everything else goes into `values`
					r.values.insert(k, v)?;
				}
			}
		}

		Ok(r)
	}

	/// Converts this `TileJSON` into a [`JsonObject`].
	///
	/// This object includes both:
	/// - Known fields (`"bounds"`, `"center"`, `"vector_layers"`)
	/// - Additional key-value pairs from `self.values`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::tilejson::*;
	/// # let tj = TileJSON::default();
	/// let json_obj = tj.as_object();
	/// ```
	pub fn as_object(&self) -> JsonObject {
		let mut obj = JsonObject::default();
		// Copy all `values` first
		for (k, v) in self.values.iter_json_values() {
			obj.set(&k, v);
		}

		let round = |x: &f64| (x * 1e6).round() / 1e6;
		let round_vec = |x: &Vec<f64>| x.iter().map(round).collect::<Vec<_>>();

		// Overwrite with known fields
		obj.set_optional("bounds", &self.bounds.map(|v| round_vec(&v.as_vec())));
		obj.set_optional("center", &self.center.map(|v| round_vec(&v.as_vec())));
		obj.set_optional("vector_layers", &self.vector_layers.as_json_value_option());
		obj.set_optional("tile_content", &self.tile_content.map(|v| v.to_string()));
		obj.set_optional("tile_format", &self.tile_format.map(|v| v.as_mime_str().to_string()));
		obj.set_optional("tile_schema", &self.tile_schema.map(|v| v.to_string()));
		obj
	}

	pub fn as_json_value(&self) -> JsonValue {
		JsonValue::Object(self.as_object())
	}

	// -------------------------------------------------------------------------
	// Conversions
	// -------------------------------------------------------------------------

	/// Returns a JSON string (pretty-printed) representing this `TileJSON`.
	pub fn as_string(&self) -> String {
		self.as_object().stringify()
	}

	/// Returns a `Blob` containing the JSON string representation.
	pub fn as_blob(&self) -> Blob {
		Blob::from(self.as_string())
	}

	pub fn as_pretty_lines(&self, max_width: usize) -> Vec<String> {
		self
			.as_object()
			.stringify_pretty_multi_line(max_width, 0)
			.split('\n')
			.map(String::from)
			.collect()
	}

	// -------------------------------------------------------------------------
	// Pyramid Integration
	// -------------------------------------------------------------------------

	/// Updates this `TileJSON` based on a [`TileBBoxPyramid`].
	///
	/// - If the pyramid includes a `GeoBBox`, intersects or sets `self.bounds` via [`limit_bbox`].
	/// - If the pyramid includes `zoom_min`, calls [`limit_min_zoom`].
	/// - If the pyramid includes `zoom_max`, calls [`limit_max_zoom`].
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

	// -------------------------------------------------------------------------
	// Getter / Setter Utilities
	// -------------------------------------------------------------------------

	/// Retrieves a `String` value from `self.values` by `key`, if present and a string.
	pub fn get_string(&self, key: &str) -> Option<String> {
		self.values.get_string(key)
	}

	/// Retrieves a string slice from `self.values` by `key`, if present and a string.
	pub fn get_str(&self, key: &str) -> Option<&str> {
		self.values.get_str(key)
	}

	/// Inserts or updates a byte (`u8`) value in `self.values`.
	pub fn set_byte(&mut self, key: &str, value: u8) -> Result<()> {
		self.values.insert(key, &JsonValue::from(value))
	}

	/// Inserts or updates a list of strings in `self.values`.
	pub fn set_list(&mut self, key: &str, value: Vec<String>) -> Result<()> {
		self.values.insert(key, &JsonValue::from(value))
	}

	/// Inserts or updates a string in `self.values`.
	pub fn set_string(&mut self, key: &str, value: &str) -> Result<()> {
		self.values.insert(key, &JsonValue::from(value))
	}

	/// Parses and sets vector layers from a [`JsonValue`].
	///
	/// # Errors
	/// Returns an error if the `JsonValue` cannot be converted into `VectorLayers`.
	pub fn set_vector_layers(&mut self, json: &JsonValue) -> Result<()> {
		self.vector_layers = VectorLayers::from_json(json).map_err(|e| anyhow!("Failed to parse vector layers: {e}"))?;
		Ok(())
	}

	// -------------------------------------------------------------------------
	// Bounds and Zoom Limits
	// -------------------------------------------------------------------------

	/// Intersects existing `self.bounds` with the given `GeoBBox` or sets it if none exists.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{tilejson::*, types::GeoBBox};
	/// let mut tj = TileJSON::default();
	/// tj.limit_bbox(GeoBBox(-180.0, -90.0, 0.0, 10.0));
	/// // If `tj.bounds` was None, now it's set; otherwise they are intersected.
	/// ```
	pub fn limit_bbox(&mut self, bbox: GeoBBox) {
		if let Some(ref mut b) = self.bounds {
			b.intersect(&bbox);
		} else {
			self.bounds = Some(bbox);
		}
	}

	/// Raises the `minzoom` value to `z` if the current `minzoom` is lower or absent.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::tilejson::*;
	/// # let mut tj = TileJSON::default();
	/// tj.set_byte("minzoom", 3).unwrap();
	/// tj.limit_min_zoom(5);
	/// // minzoom is now 5
	/// ```
	pub fn limit_min_zoom(&mut self, z: u8) {
		self.values.update_byte("minzoom", |mz| mz.map_or(z, |mz| mz.max(z)));
	}

	/// Lowers the `maxzoom` value to `z` if the current `maxzoom` is higher or absent.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::tilejson::*;
	/// # let mut tj = TileJSON::default();
	/// tj.set_byte("maxzoom", 15).unwrap();
	/// tj.limit_max_zoom(10);
	/// // maxzoom is now 10
	/// ```
	pub fn limit_max_zoom(&mut self, z: u8) {
		self.values.update_byte("maxzoom", |mz| mz.map_or(z, |mz| mz.min(z)));
	}

	// -------------------------------------------------------------------------
	// Merging
	// -------------------------------------------------------------------------

	/// Merges `other` into this `TileJSON` with specific rules:
	/// 1. **Bounds**: extends or sets `self.bounds` if `other.bounds` is present.
	/// 2. **Center**: overwrites `self.center` if `other.center` is `Some`.
	/// 3. **minzoom** / **maxzoom**: uses the min or max across the two.
	/// 4. **Other values**: overwrites conflicts from `other.values`.
	/// 5. **Vector layers**: merges layers from `other`, overwriting existing layer IDs if needed.
	///
	/// # Errors
	/// May fail if inserting into `self.values` fails (e.g., invalid data).
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

		// 3. Merge minzoom/maxzoom
		if let Some(omin) = other.values.get_byte("minzoom") {
			let new_min = self.values.get_byte("minzoom").map_or(omin, |mz| mz.min(omin));
			self.values.insert("minzoom", &JsonValue::from(new_min))?;
		}
		if let Some(omax) = other.values.get_byte("maxzoom") {
			let new_max = self.values.get_byte("maxzoom").map_or(omax, |mz| mz.max(omax));
			self.values.insert("maxzoom", &JsonValue::from(new_max))?;
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

	pub fn update_from_reader_parameters(&mut self, rp: &TilesReaderParameters) {
		self.update_from_pyramid(&rp.bbox_pyramid);

		self.tile_format = Some(rp.tile_format);

		self.tile_content = self.tile_format.map(|f| f.get_type());

		if let Some(tile_content) = self.tile_content {
			if self.tile_schema.map(|s| s.get_tile_content()) != self.tile_content {
				self.tile_schema = Some(match tile_content {
					TileType::Raster => TileSchema::RasterRGB,
					TileType::Vector => self.vector_layers.get_tile_schema(),
					TileType::Unknown => TileSchema::Unknown,
				});
			}
		}
	}

	// -------------------------------------------------------------------------
	// Validation
	// -------------------------------------------------------------------------

	/// Validates basic fields according to the TileJSON 3.0.0 specification.
	///
	/// Checks:
	/// - `"tilejson"` pattern `^[123]\.[012]\.[01]$`
	/// - optional lists and strings are valid if present
	/// - optional numeric fields (bounds, center) are in valid ranges
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
		// 3.3 vector_layers handled separately in `check_vector` or `check_raster`.

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

	/// Validates that this `TileJSON` is correct for a **raster** tileset.
	///
	/// - Must pass `check_basics()`.
	/// - Must not have `vector_layers`.
	pub fn check_raster(&self) -> Result<()> {
		self.check_basics()?;
		ensure!(
			self.vector_layers.0.is_empty(),
			"Raster tilesets must not have 'vector_layers'"
		);
		Ok(())
	}

	/// Validates that this `TileJSON` is correct for a **vector** tileset.
	///
	/// - Must pass `check_basics()`.
	/// - Must have at least one `vector_layer`.
	/// - Layers themselves must pass checks.
	pub fn check_vector(&self) -> Result<()> {
		self.check_basics()?;
		ensure!(
			!self.vector_layers.0.is_empty(),
			"Vector tilesets must have 'vector_layers'"
		);
		self.vector_layers.check()?;
		Ok(())
	}

	// -------------------------------------------------------------------------
	// Final Utilities
	// -------------------------------------------------------------------------

	/// Converts this `TileJSON` to a JSON string (synonym for [`Self::as_string`]).
	pub fn stringify(&self) -> String {
		self.as_string()
	}

	pub fn try_from_blob_or_default(blob: &Blob) -> TileJSON {
		TileJSON::try_from(blob.as_str()).unwrap_or_else(|e| {
			eprintln!("Failed to parse TileJSON: {e}");
			eprintln!("Use default TileJSON instead");
			TileJSON::default()
		})
	}
}

// ----------------------------------------------------------------------------
// Implementations for conversions
// ----------------------------------------------------------------------------

impl TryFrom<&str> for TileJSON {
	type Error = anyhow::Error;

	/// Parses a JSON string to build a `TileJSON`.
	///
	/// # Errors
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
		// Provide a short debug with JSON representation
		write!(f, "TileJSON({})", self.as_string())
	}
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;

	/// Creates a minimal valid TileJSON object in the form of `JsonObject`.
	fn make_test_json_object() -> JsonObject {
		let mut obj = JsonObject::default();
		// "tilejson" is required by the spec
		obj.set("tilejson", JsonValue::from("3.0.0"));
		// Minimal valid fields
		obj.set("bounds", JsonValue::from(vec![-180.0, -90.0, 180.0, 90.0]));
		obj.set("center", JsonValue::from(vec![0.0, 0.0, 3.0]));
		obj
	}

	#[test]
	fn should_parse_basic_tilejson_from_object() -> Result<()> {
		let obj = make_test_json_object();
		let tj = TileJSON::from_object(&obj)?;
		assert!(tj.bounds.is_some(), "Expected bounds to be set");
		assert!(tj.center.is_some(), "Expected center to be set");
		assert_eq!(tj.values.get_string("tilejson"), Some("3.0.0".to_string()));
		Ok(())
	}

	#[test]
	fn should_check_raster_tilejson_without_vector_layers() -> Result<()> {
		let obj = make_test_json_object();
		let tj = TileJSON::from_object(&obj)?;
		// Should pass as a raster tilejson
		assert!(tj.check_raster().is_ok());
		Ok(())
	}

	#[test]
	fn should_fail_check_vector_if_no_vector_layers() -> Result<()> {
		let obj = make_test_json_object();
		let tj = TileJSON::from_object(&obj)?;
		let result = tj.check_vector();
		assert!(result.is_err(), "Expected error if no vector layers");
		Ok(())
	}

	#[test]
	fn should_merge_minmaxzoom_correctly() -> Result<()> {
		let mut tj1 = TileJSON::default();
		tj1.set_byte("minzoom", 5)?;
		tj1.set_byte("maxzoom", 15)?;

		let mut tj2 = TileJSON::default();
		tj2.set_byte("minzoom", 2)?;
		tj2.set_byte("maxzoom", 20)?;

		tj1.merge(&tj2)?;
		// minzoom becomes min(5,2) => 2, maxzoom => max(15,20) => 20
		assert_eq!(tj1.values.get_byte("minzoom"), Some(2));
		assert_eq!(tj1.values.get_byte("maxzoom"), Some(20));
		Ok(())
	}

	#[test]
	fn should_intersect_existing_bounds_with_given_bbox() {
		let mut tj = TileJSON::default();
		let existing_bbox = GeoBBox(-10.0, -5.0, 10.0, 5.0);
		let new_bbox = GeoBBox(-15.0, -10.0, 0.0, 2.0);
		tj.bounds = Some(existing_bbox);
		tj.limit_bbox(new_bbox);

		// Intersection => [-10, -5, 0, 2]
		let b = tj.bounds.expect("Should have bounds");
		assert_eq!(b.as_array(), [-10.0, -5.0, 0.0, 2.0]);
	}

	#[test]
	fn should_update_from_pyramid_and_set_bounds_and_zoom() {
		let mut tj = TileJSON::default();
		// If we have no bounds, it should set them. If we have no minzoom/maxzoom, it sets them.
		let bbox_pyramid = TileBBoxPyramid::from_geo_bbox(2, 12, &GeoBBox(-180.0, -90.0, 180.0, 90.0));
		tj.update_from_pyramid(&bbox_pyramid);

		// Bounds
		let bounds = tj.bounds.expect("Should have updated bounds");
		// Typically from_geo_bbox can clamp lat/long (like -85.051...), adjust test if relevant
		// This depends on the implementation within `TileBBoxPyramid`.
		assert_eq!(
			bounds.as_array(),
			[-180.0, -85.05112877980659, 180.0, 85.05112877980659]
		);

		// Zoom
		assert_eq!(tj.values.get_byte("minzoom"), Some(2));
		assert_eq!(tj.values.get_byte("maxzoom"), Some(12));
	}

	#[test]
	fn should_parse_valid_tilejson_from_string() -> Result<()> {
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
	fn should_fail_raster_check_if_vector_layers_exist() -> Result<()> {
		let mut obj = make_test_json_object();
		// Simulate vector_layers
		let mut vl_obj = JsonObject::default();
		vl_obj.set("id", JsonValue::from("layer1"));
		let vector_json = JsonValue::from(vec![JsonValue::Object(vl_obj)]);
		obj.set("vector_layers", vector_json);

		let tj = TileJSON::from_object(&obj)?;
		let res = tj.check_raster();
		assert!(res.is_err(), "Raster cannot have vector_layers");
		Ok(())
	}

	#[test]
	fn should_debug_print_as_json() {
		let tj = TileJSON::default();
		let debug_str = format!("{tj:?}");
		assert!(
			debug_str.contains("TileJSON("),
			"Debug string should contain 'TileJSON(' prefix"
		);
	}
}
