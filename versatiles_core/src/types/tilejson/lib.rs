//! This module defines the `TileJSON` struct, representing a `TileJSON` object and its fields.
//!
//! A `TileJSON` can contain:
//! - An optional geographic bounding box, `[west, south, east, north]`.
//! - An optional geographic center, `[longitude, latitude, zoom_level]`.
//! - Additional `TileJSON` key-value pairs in [`TileJsonValues`].
//! - A collection of vector layers defined.
//!
//! Methods are provided to parse from JSON, merge with other `TileJSON` objects,
//! and validate according to the `TileJSON` 3.0.0 specification.
//!
//! # Example
//! ```rust
//! # use versatiles_core::TileJSON;
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
//! let json_string = tilejson.stringify();
//! let json_blob = tilejson.as_blob();
//! # Ok(())
//! # }
//! ```

use super::{TileJsonValues, VectorLayers};
use crate::{
	Blob, GeoBBox, GeoCenter, PyramidInfo, TileFormat, TileSchema, TileSize, TileType,
	json::{JsonObject, JsonValue, parse_json_str},
};
use anyhow::{Ok, Result, anyhow, ensure};
use regex::Regex;
use std::fmt::Debug;

/// A struct representing a `TileJSON` object.
///
/// # Fields
/// - `bounds`: An optional geographic bounding box (`[west, south, east, north]`).
/// - `center`: An optional geographic center (`[lon, lat, zoom]`).
/// - `values`: A flexible map of additional `TileJSON` key-value pairs.
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
	/// Optional tile content type derived from format (raster/vector/unknown).
	pub tile_type: Option<TileType>,
	/// Optional tile format (e.g., "image/png", "application/x-protobuf").
	pub tile_format: Option<TileFormat>,
	/// Optional tile schema describing the expected layer/attribute structure.
	pub tile_schema: Option<TileSchema>,
	/// Optional tile size in pixels (typically 256 or 512).
	pub tile_size: Option<TileSize>,
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
	/// - `"vector_layers"`: Interpreted as vector layers.
	/// - `"tile_type"`: Interpreted as [`TileType`].
	/// - `"tile_format"`: Interpreted as [`TileFormat`] (MIME-like strings).
	/// - `"tile_schema"`: Interpreted as [`TileSchema`].
	/// - `"tile_size"`: Interpreted as [`TileSize`].
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
				"tile_type" => {
					r.tile_type = Some(TileType::try_from(v.as_str()?)?);
				}
				"tile_format" => {
					r.tile_format = Some(TileFormat::try_from_mime(v.as_str()?)?);
				}
				"tile_schema" => {
					r.tile_schema = Some(TileSchema::try_from(v.as_str()?)?);
				}
				"tile_size" => {
					r.tile_size = Some(TileSize::try_from(v.as_number()?)?);
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
	/// # use versatiles_core::TileJSON;
	/// # let tj = TileJSON::default();
	/// let json_obj = tj.as_object();
	/// ```
	#[must_use]
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
		obj.set_optional("tile_type", &self.tile_type.map(|v| v.to_string()));
		obj.set_optional("tile_format", &self.tile_format.map(|v| v.as_mime_str().to_string()));
		obj.set_optional("tile_schema", &self.tile_schema.map(|v| v.to_string()));
		obj.set_optional("tile_size", &self.tile_size.map(|v| v.size()));
		obj
	}

	#[must_use]
	pub fn as_json_value(&self) -> JsonValue {
		JsonValue::Object(self.as_object())
	}

	// -------------------------------------------------------------------------
	// Conversions
	// -------------------------------------------------------------------------

	/// Returns a `Blob` containing the JSON string representation.
	#[must_use]
	pub fn as_blob(&self) -> Blob {
		Blob::from(self.stringify())
	}

	/// Pretty-prints this `TileJSON` into multiple lines with a maximum width.
	///
	/// This is useful for CLI or log output where compact, readable wrapping is preferred.
	///
	/// # Arguments
	/// * `max_width` — Target maximum line width. Long values may still exceed it.
	///
	/// # Returns
	/// A vector of lines representing the pretty-printed JSON.
	pub fn to_pretty_lines(&self, max_width: usize) -> Vec<String> {
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

	/// Updates this `TileJSON` based on any type implementing [`PyramidInfo`],
	/// using pyramid values only as fallback.
	///
	/// - Sets `bounds` from the pyramid only if `self.bounds` is `None`.
	/// - Sets `minzoom` from the pyramid.
	/// - Sets `maxzoom` from the pyramid.
	///
	/// Any type implementing [`PyramidInfo`] can be passed here (e.g. [`crate::TilePyramid`]).
	pub fn update_from_pyramid<P: PyramidInfo>(&mut self, pyramid: &P) {
		if self.bounds.is_none() {
			self.bounds = pyramid.geo_bbox();
		}

		if let Some(z) = pyramid.level_min() {
			self.set_zoom_min(z);
		}

		if let Some(z) = pyramid.level_max() {
			self.set_zoom_max(z);
		}
	}

	// -------------------------------------------------------------------------
	// Getter / Setter Utilities
	// -------------------------------------------------------------------------

	/// Retrieves a `String` value from `self.values` by `key`, if present and a string.
	#[must_use]
	pub fn string(&self, key: &str) -> Option<String> {
		self.values.string(key)
	}

	/// Retrieves a string slice from `self.values` by `key`, if present and a string.
	#[must_use]
	pub fn str(&self, key: &str) -> Option<&str> {
		self.values.str(key)
	}

	/// Retrieves an `i64` value from `self.values` by `key`, if present and an integer.
	#[must_use]
	pub fn integer(&self, key: &str) -> Option<i64> {
		self.values.integer(key)
	}

	/// Removes the value associated with `key` from `self.values`, returning `true` if it was present.
	pub fn remove(&mut self, key: &str) -> bool {
		self.values.remove(key)
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
	/// # use versatiles_core::{TileJSON, GeoBBox};
	/// let mut tj = TileJSON::default();
	/// tj.limit_bbox(GeoBBox::new(-180.0, -90.0, 0.0, 10.0).unwrap());
	/// // If `tj.bounds` was None, now it's set; otherwise they are intersected.
	/// ```
	pub fn limit_bbox(&mut self, bbox: GeoBBox) {
		if let Some(ref mut b) = self.bounds {
			b.intersect(&bbox);
		} else {
			self.bounds = Some(bbox);
		}
	}

	#[must_use]
	pub fn zoom_min(&self) -> Option<u8> {
		self.values.integer("minzoom").and_then(|z| u8::try_from(z).ok())
	}

	/// Sets the `minzoom` value to `z`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::TileJSON;
	/// # let mut tj = TileJSON::default();
	/// tj.set_zoom_min(5);
	/// // minzoom is now 5
	/// ```
	pub fn set_zoom_min(&mut self, z: u8) {
		self.values.set("minzoom", z);
	}

	#[must_use]
	pub fn zoom_max(&self) -> Option<u8> {
		self.values.integer("maxzoom").and_then(|z| u8::try_from(z).ok())
	}

	/// Sets the `maxzoom` value to `z`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::TileJSON;
	/// # let mut tj = TileJSON::default();
	/// tj.set_zoom_max(10);
	/// // maxzoom is now 10
	/// ```
	pub fn set_zoom_max(&mut self, z: u8) {
		self.values.set("maxzoom", z);
	}

	pub fn set_tile_size(&mut self, size: u32) -> Result<()> {
		self.tile_size = Some(TileSize::try_from(size)?);
		Ok(())
	}

	// -------------------------------------------------------------------------
	// Merging
	// -------------------------------------------------------------------------

	/// Value keys that are **combined** (unioned) when merging instead of being
	/// overwritten, paired with the separator used to join them. This keeps every
	/// source's credit/description when several tilesets are merged — overwriting
	/// `attribution` in particular would silently drop required source credits.
	const COMBINE_SEPARATORS: &'static [(&'static str, &'static str)] = &[("attribution", " · "), ("description", "\n")];

	/// Merges several `TileJSON`s into one, in iteration order.
	///
	/// This is the primitive that [`merge`](Self::merge) delegates to; prefer it
	/// when combining the metadata of multiple sources (e.g. multi-source `from_`
	/// pipeline operations) so the call sites don't need their own merge loop.
	///
	/// Merge rules: see [`merge`](Self::merge). `tile_type` / `tile_format` /
	/// `tile_schema` / `tile_size` are **not** derived here — those come from the
	/// source's [`TileSourceMetadata`](crate) and are applied separately.
	///
	/// # Errors
	/// Propagates any error from the underlying pairwise merge.
	pub fn merge_all<'a, I>(items: I) -> Result<TileJSON>
	where
		I: IntoIterator<Item = &'a TileJSON>,
	{
		let mut merged = TileJSON::default();
		for item in items {
			merged.merge(item)?;
		}
		Ok(merged)
	}

	/// Merges `other` into this `TileJSON` with specific rules:
	/// 1. **Bounds**: extends or sets `self.bounds` if `other.bounds` is present.
	/// 2. **Center**: overwrites `self.center` if `other.center` is `Some`.
	/// 3. **minzoom** / **maxzoom**: uses the min or max across the two.
	/// 4. **`attribution` / `description`**: combined (unioned, de-duplicated) so
	///    no source's credit or description is lost.
	/// 5. **Other values**: overwrites conflicts from `other.values`.
	/// 6. **Vector layers**: merges layers from `other`, overwriting existing layer IDs if needed.
	///
	/// To merge more than two, use [`merge_all`](Self::merge_all).
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
		if let Some(omin) = other.zoom_min() {
			let new_min = self.zoom_min().map_or(omin, |mz| mz.min(omin));
			self.set_zoom_min(new_min);
		}
		if let Some(omax) = other.zoom_max() {
			let new_max = self.zoom_max().map_or(omax, |mz| mz.max(omax));
			self.set_zoom_max(new_max);
		}

		// 4./5. Merge remaining values: combine the union-keys, overwrite the rest.
		for (k, v) in other.values.iter_json_values() {
			if k == "minzoom" || k == "maxzoom" {
				continue;
			}
			if let Some((_, separator)) = Self::COMBINE_SEPARATORS.iter().find(|(key, _)| *key == k) {
				if let Some(combined) = combine_values(
					self.values.string(&k).as_deref(),
					other.values.string(&k).as_deref(),
					separator,
				) {
					self.values.insert(&k, &JsonValue::from(combined.as_str()))?;
				}
				continue;
			}
			self.values.insert(&k, &v)?;
		}

		// 6. Merge vector_layers
		self.vector_layers.merge(&other.vector_layers)?;
		Ok(())
	}

	// -------------------------------------------------------------------------
	// Validation
	// -------------------------------------------------------------------------

	/// Validates basic fields according to the `TileJSON` 3.0.0 specification.
	///
	/// Checks:
	/// - `"tilejson"` pattern `^[123]\.[012]\.[01]$`
	/// - optional lists and strings are valid if present
	/// - optional numeric fields (bounds, center) are in valid ranges
	fn check_basics(&self) -> Result<()> {
		// 3.1 tilejson - required
		let version = self
			.values
			.string("tilejson")
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

		// 3.6 center - optional
		if let Some(c) = self.center {
			c.check()?;
		}

		// 3.7 data - optional
		self.values.check_optional_list("data")?;

		// 3.8 description - optional
		self.values.check_optional_string("description")?;

		// 3.9 fillzoom - optional
		self.values.check_optional_integer("fillzoom")?;

		// 3.10 grids - optional
		self.values.check_optional_list("grids")?;

		// 3.11 legend - optional
		self.values.check_optional_string("legend")?;

		// 3.12 maxzoom - optional
		self.values.check_optional_integer("maxzoom")?;

		// 3.13 minzoom - optional
		self.values.check_optional_integer("minzoom")?;

		// 3.14 name - optional
		self.values.check_optional_string("name")?;

		// 3.15 scheme - optional
		self.values.check_optional_string("scheme")?;

		// 3.16 template - optional
		self.values.check_optional_string("template")?;

		// 3.17 version - optional
		if let Some(v) = self.values.string("version") {
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
		self.vector_layers.check()
	}

	// -------------------------------------------------------------------------
	// Final Utilities
	// -------------------------------------------------------------------------

	/// Converts this `TileJSON` to a JSON string.
	#[must_use]
	pub fn stringify(&self) -> String {
		self.as_object().stringify()
	}

	/// Parses `TileJSON` from a blob or returns `TileJSON::default()` on failure.
	///
	/// Logs a warning with the parse error and falls back to a minimal default.
	///
	/// # Returns
	/// A valid `TileJSON` even if the input is invalid.
	#[must_use]
	pub fn try_from_blob_or_default(blob: &Blob) -> TileJSON {
		TileJSON::try_from(blob.as_str()).unwrap_or_else(|e| {
			log::warn!("Failed to parse TileJSON: {e}");
			log::warn!("Use default TileJSON instead");
			TileJSON::default()
		})
	}
}

/// Combines two text values into a separator-joined union, skipping empty inputs
/// and not re-appending an `addition` that is already present as a segment of
/// `current`. Used by [`TileJSON::merge`] for keys like `attribution` so that
/// merging many sources accumulates every distinct credit exactly once.
fn combine_values(current: Option<&str>, addition: Option<&str>, separator: &str) -> Option<String> {
	let current = current.map(str::trim).filter(|s| !s.is_empty());
	let addition = addition.map(str::trim).filter(|s| !s.is_empty());
	match (current, addition) {
		(None, None) => None,
		(Some(value), None) | (None, Some(value)) => Some(value.to_string()),
		(Some(current), Some(addition)) => {
			if current.split(separator).any(|segment| segment == addition) {
				Some(current.to_string())
			} else {
				Some(format!("{current}{separator}{addition}"))
			}
		}
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
		let object = parse_json_str(text)?.into_object()?;
		TileJSON::from_object(&object)
	}
}

impl TryFrom<Vec<u8>> for TileJSON {
	type Error = anyhow::Error;

	fn try_from(blob: Vec<u8>) -> Result<TileJSON> {
		TileJSON::try_from(std::str::from_utf8(&blob)?)
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
		write!(f, "TileJSON({})", self.stringify())
	}
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TilePyramid;
	use approx::assert_relative_eq;

	/// Creates a minimal valid `TileJSON` object in the form of `JsonObject`.
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
		assert_eq!(tj.values.string("tilejson"), Some("3.0.0".to_string()));
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
		tj1.set_zoom_min(5);
		tj1.set_zoom_max(15);

		let mut tj2 = TileJSON::default();
		tj2.set_zoom_min(2);
		tj2.set_zoom_max(20);

		tj1.merge(&tj2)?;
		// minzoom becomes min(5,2) => 2, maxzoom => max(15,20) => 20
		assert_eq!(tj1.zoom_min(), Some(2));
		assert_eq!(tj1.zoom_max(), Some(20));
		Ok(())
	}

	#[test]
	fn should_intersect_existing_bounds_with_given_bbox() {
		let mut tj = TileJSON::default();
		let existing_bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
		let new_bbox = GeoBBox::new(-15.0, -10.0, 0.0, 2.0).unwrap();
		tj.bounds = Some(existing_bbox);
		tj.limit_bbox(new_bbox);

		// Intersection => [-10, -5, 0, 2]
		let b = tj.bounds.expect("Should have bounds");
		assert_relative_eq!(b.as_array().as_slice(), [-10.0_f64, -5.0, 0.0, 2.0].as_slice());
	}

	#[test]
	fn should_update_from_pyramid_and_set_bounds_and_zoom() {
		let mut tj = TileJSON::default();
		// If we have no bounds, it should set them. If we have no minzoom/maxzoom, it sets them.
		let tile_pyramid = TilePyramid::from_geo_bbox(2, 12, &GeoBBox::new(-180.0, -90.0, 180.0, 90.0).unwrap()).unwrap();
		tj.update_from_pyramid(&tile_pyramid);

		// Bounds
		let bounds = tj.bounds.expect("Should have updated bounds");
		// Typically from_geo_bbox can clamp lat/long (like -85.051...), adjust test if relevant
		// This depends on the implementation within `TilePyramid`.
		assert_relative_eq!(
			bounds.as_array().as_slice(),
			[-180.0_f64, -85.05112877980659, 180.0, 85.05112877980659].as_slice()
		);

		// Zoom
		assert_eq!(tj.zoom_min(), Some(2));
		assert_eq!(tj.zoom_max(), Some(12));
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
		assert_eq!(tj.values.string("tilejson"), Some("3.0.0".to_string()));
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
		assert!(debug_str.contains("TileJSON("));
	}

	#[test]
	fn should_roundtrip_via_object() -> Result<()> {
		let obj = make_test_json_object();
		let tj1 = TileJSON::from_object(&obj)?;
		let obj2 = tj1.as_object();
		let tj2 = TileJSON::from_object(&obj2)?;
		assert_eq!(tj1, tj2);
		Ok(())
	}

	#[test]
	fn should_roundtrip_via_string_and_blob() -> Result<()> {
		let obj = make_test_json_object();
		let tj1 = TileJSON::from_object(&obj)?;
		let json_str = tj1.stringify();
		let tj2 = TileJSON::try_from(json_str.as_str())?;
		assert_eq!(tj1, tj2);

		let blob = tj1.as_blob();
		let tj3 = TileJSON::try_from(&blob)?;
		assert_eq!(tj1, tj3);
		Ok(())
	}

	#[test]
	fn should_return_pretty_lines() -> Result<()> {
		let obj = make_test_json_object();
		let tj = TileJSON::from_object(&obj)?;
		let lines = tj.to_pretty_lines(40);
		assert!(lines.len() > 1);
		Ok(())
	}

	#[test]
	fn should_try_from_blob_or_default_return_default_on_invalid_json() {
		let blob = Blob::from("{ invalid json");
		let tj = TileJSON::try_from_blob_or_default(&blob);
		assert_eq!(tj, TileJSON::default());
	}

	#[test]
	fn should_set_and_get_string_and_str() -> Result<()> {
		let mut tj = TileJSON::default();
		tj.set_string("key", "value")?;
		assert_eq!(tj.string("key"), Some("value".to_string()));
		assert_eq!(tj.str("key"), Some("value"));
		Ok(())
	}

	#[test]
	fn should_set_and_get_byte() -> Result<()> {
		let mut tj = TileJSON::default();
		tj.set_byte("byte_key", 42)?;
		assert_eq!(tj.values.integer("byte_key"), Some(42));
		Ok(())
	}

	#[test]
	fn should_set_min_and_max_zoom_correctly() {
		let mut tj = TileJSON::default();
		tj.set_zoom_min(3);
		assert_eq!(tj.values.integer("minzoom"), Some(3));
		// Lower minzoom should not decrease the value
		tj.set_zoom_min(1);
		assert_eq!(tj.values.integer("minzoom"), Some(1));

		let mut tj2 = TileJSON::default();
		tj2.set_zoom_max(10);
		assert_eq!(tj2.zoom_max(), Some(10));
		// Higher maxzoom should not increase the value
		tj2.set_zoom_max(20);
		assert_eq!(tj2.zoom_max(), Some(20));
	}

	#[test]
	fn should_merge_bounds_center_and_additional_values() -> Result<()> {
		let mut tj1 = TileJSON {
			bounds: Some(GeoBBox::new(0.0, 0.0, 5.0, 5.0).unwrap()),
			center: Some(GeoCenter(1.0, 1.0, 2)),
			..Default::default()
		};
		tj1.set_string("foo", "bar")?;

		let mut tj2 = TileJSON {
			bounds: Some(GeoBBox::new(-5.0, -5.0, 3.0, 3.0).unwrap()),
			center: Some(GeoCenter(2.0, 2.0, 4)),
			..Default::default()
		};
		tj2.set_string("baz", "qux")?;

		tj1.merge(&tj2)?;

		// Bounds should be the union of both
		assert_eq!(tj1.bounds, Some(GeoBBox::new(-5.0, -5.0, 5.0, 5.0).unwrap()));
		// Center should be overwritten by the other
		assert_eq!(tj1.center, Some(GeoCenter(2.0, 2.0, 4)));
		// Original value should remain, and new value should be inserted
		assert_eq!(tj1.values.string("foo"), Some("bar".to_string()));
		assert_eq!(tj1.values.string("baz"), Some("qux".to_string()));
		Ok(())
	}

	#[test]
	fn should_combine_attribution_and_description_instead_of_overwriting() -> Result<()> {
		let mut a = TileJSON::default();
		a.set_string("attribution", "© OpenStreetMap")?;
		a.set_string("description", "Base map")?;
		a.set_string("name", "first")?;

		let mut b = TileJSON::default();
		b.set_string("attribution", "© Provider B")?;
		b.set_string("description", "Overlay")?;
		b.set_string("name", "second")?;

		a.merge(&b)?;

		// attribution/description are unioned so no source's credit is lost…
		assert_eq!(
			a.string("attribution"),
			Some("© OpenStreetMap · © Provider B".to_string())
		);
		assert_eq!(a.string("description"), Some("Base map\nOverlay".to_string()));
		// …while ordinary scalars still follow last-write-wins.
		assert_eq!(a.string("name"), Some("second".to_string()));
		Ok(())
	}

	#[test]
	fn should_dedup_identical_attribution_when_merging() -> Result<()> {
		let mut shared = TileJSON::default();
		shared.set_string("attribution", "© OpenStreetMap")?;

		let same = shared.clone();
		shared.merge(&same)?;

		// Identical credit must not be duplicated.
		assert_eq!(shared.string("attribution"), Some("© OpenStreetMap".to_string()));
		Ok(())
	}

	#[test]
	fn merge_all_unions_attribution_across_many_sources_and_dedups() -> Result<()> {
		let attribution = |s: &str| -> Result<TileJSON> {
			let mut tj = TileJSON::default();
			tj.set_string("attribution", s)?;
			Ok(tj)
		};

		// Three sources, two of which share a credit.
		let a = attribution("© A")?;
		let b = attribution("© B")?;
		let a2 = attribution("© A")?;

		let merged = TileJSON::merge_all([&a, &b, &a2])?;
		assert_eq!(merged.string("attribution"), Some("© A · © B".to_string()));

		// Empty iterator yields a default TileJSON.
		assert_eq!(TileJSON::merge_all(std::iter::empty())?, TileJSON::default());
		Ok(())
	}

	/// Builds a `vector_layers` layer object (`id` + `fields` [+ optional zooms]).
	fn vlayer_json(id: &str, fields: &[(&str, &str)], minzoom: Option<u8>, maxzoom: Option<u8>) -> JsonValue {
		let mut field_obj = JsonObject::default();
		for (k, v) in fields {
			field_obj.set(k, JsonValue::from(*v));
		}
		let mut obj = JsonObject::default();
		obj.set("id", JsonValue::from(id));
		obj.set("fields", JsonValue::Object(field_obj));
		if let Some(z) = minzoom {
			obj.set("minzoom", JsonValue::from(z));
		}
		if let Some(z) = maxzoom {
			obj.set("maxzoom", JsonValue::from(z));
		}
		JsonValue::Object(obj)
	}

	fn tj_with_layers(layers: Vec<JsonValue>) -> Result<TileJSON> {
		let mut tj = TileJSON::default();
		tj.set_vector_layers(&JsonValue::from(layers))?;
		Ok(tj)
	}

	#[test]
	fn should_merge_vector_layers_through_tilejson() -> Result<()> {
		let mut a = tj_with_layers(vec![vlayer_json("roads", &[("name", "String")], Some(5), Some(10))])?;
		let b = tj_with_layers(vec![
			vlayer_json("roads", &[("surface", "String")], Some(3), Some(12)),
			vlayer_json("water", &[("kind", "String")], None, None),
		])?;

		a.merge(&b)?;

		// Overlapping "roads": fields unioned, zoom range widened.
		let roads = a.vector_layers.find("roads").expect("roads layer present");
		assert!(roads.fields.contains_key("name"));
		assert!(roads.fields.contains_key("surface"));
		assert_eq!(roads.minzoom, Some(3));
		assert_eq!(roads.maxzoom, Some(12));
		// Distinct "water" carried over.
		assert!(a.vector_layers.find("water").is_some());
		Ok(())
	}

	#[test]
	fn merge_all_accumulates_vector_layers_across_sources() -> Result<()> {
		let a = tj_with_layers(vec![vlayer_json("roads", &[("a", "String")], Some(4), Some(8))])?;
		let b = tj_with_layers(vec![
			vlayer_json("roads", &[("b", "String")], Some(2), Some(9)),
			vlayer_json("water", &[("w", "String")], None, None),
		])?;
		let c = tj_with_layers(vec![
			vlayer_json("roads", &[("c", "String")], Some(6), Some(14)),
			vlayer_json("places", &[("p", "String")], None, None),
		])?;

		let merged = TileJSON::merge_all([&a, &b, &c])?;

		// Every distinct layer id is present.
		let mut ids = merged.vector_layers.layer_ids();
		ids.sort();
		assert_eq!(
			ids,
			vec!["places".to_string(), "roads".to_string(), "water".to_string()]
		);

		// "roads" accumulates fields and zoom range across all three sources.
		let roads = merged.vector_layers.find("roads").expect("roads layer present");
		assert!(roads.fields.contains_key("a"));
		assert!(roads.fields.contains_key("b"));
		assert!(roads.fields.contains_key("c"));
		assert_eq!(roads.minzoom, Some(2)); // min(4, 2, 6)
		assert_eq!(roads.maxzoom, Some(14)); // max(8, 9, 14)
		Ok(())
	}

	#[test]
	fn should_merge_same_layer_split_across_zoom_ranges() -> Result<()> {
		// The "roads" layer is split across sources by zoom: one source covers the
		// low zooms (0..=8), the other the high zooms (9..=14), with adjacent /
		// disjoint ranges. The merged layer must span the full 0..=14 range and
		// keep both sources' fields, as a single combined entry.
		let low = tj_with_layers(vec![vlayer_json("roads", &[("name", "String")], Some(0), Some(8))])?;
		let high = tj_with_layers(vec![vlayer_json("roads", &[("ref", "String")], Some(9), Some(14))])?;

		let merged = TileJSON::merge_all([&low, &high])?;

		assert_eq!(merged.vector_layers.layer_ids(), vec!["roads".to_string()]);
		let roads = merged.vector_layers.find("roads").expect("roads layer present");
		assert_eq!(roads.minzoom, Some(0));
		assert_eq!(roads.maxzoom, Some(14));
		assert!(roads.fields.contains_key("name"));
		assert!(roads.fields.contains_key("ref"));
		Ok(())
	}

	#[test]
	fn should_return_none_for_missing_getters() {
		let tj = TileJSON::default();
		assert_eq!(tj.string("missing"), None);
		assert_eq!(tj.str("missing"), None);
	}

	#[test]
	fn should_set_and_retrieve_list() -> Result<()> {
		let mut tj = TileJSON::default();
		let list = vec!["a".to_string(), "b".to_string()];
		tj.set_list("list_key", list.clone())?;
		// Inspect via as_json_value
		let obj = tj.as_json_value().into_object()?;
		let arr = obj.get("list_key").unwrap().as_array()?;
		assert_eq!(arr.to_string_vec()?, list);
		Ok(())
	}

	#[test]
	fn should_set_vector_layers_from_json() -> Result<()> {
		let mut tj = TileJSON::default();

		// Build a single layer JSON
		let mut layer_obj = JsonObject::default();
		layer_obj.set("id", JsonValue::from("layer1"));
		layer_obj.set("fields", JsonValue::new_object());
		let json = JsonValue::from(vec![JsonValue::Object(layer_obj.clone())]);
		tj.set_vector_layers(&json)?;

		// Verify via as_object
		let obj = tj.as_object();
		let arr = obj.get("vector_layers").unwrap().as_array()?;
		assert_eq!(arr.as_vec(), &vec![JsonValue::Object(layer_obj)]);
		Ok(())
	}

	#[test]
	fn should_return_json_value_as_object() {
		let tj = TileJSON::default();
		let json_value = tj.as_json_value();
		let obj = json_value.into_object().unwrap();
		assert_eq!(obj, tj.as_object());
	}

	#[test]
	fn should_stringify_same_as_as_string() {
		let tj = TileJSON::default();
		assert_eq!(tj.stringify(), tj.stringify());
	}

	#[test]
	fn should_parse_from_string_reference() -> Result<()> {
		let json_str = r#"{"tilejson":"3.0.0","bounds":[-180,-90,180,90],"center":[0.0,0.0,3.0]}"#.to_string();
		let tj = TileJSON::try_from(&json_str)?;
		assert_eq!(tj.values.string("tilejson"), Some("3.0.0".to_string()));
		Ok(())
	}

	#[test]
	fn should_convert_into_string_and_blob() {
		let tj = TileJSON::default();
		let s: String = tj.clone().into();
		assert_eq!(s, tj.stringify());
		let blob: Blob = tj.clone().into();
		assert_eq!(blob.as_str(), tj.stringify());
		let blob_ref: Blob = (&tj).into();
		assert_eq!(blob_ref.as_str(), tj.stringify());
	}
}
