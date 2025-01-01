use super::{value::TileJsonValues, vector_layer::VectorLayers};
use crate::{
	types::{Blob, GeoBBox, GeoCenter, TileBBoxPyramid},
	utils::{parse_json_str, JsonObject, JsonValue},
};
use anyhow::{ensure, Result};
use regex::Regex;

/// A struct representing a TileJSON object.
///
/// Fields:
/// - `bounds`: An optional geographic bounding box (`[west, south, east, north]`).
/// - `center`: An optional geographic center (`[lon, lat, zoom]`).
/// - `values`: A flexible map of additional TileJSON key-value pairs.
/// - `vector_layers`: A structured set of vector layer definitions.
#[derive(Clone, Debug, PartialEq)]
#[derive(Default)]
pub struct TileJSON {
	pub bounds: Option<GeoBBox>,
	pub center: Option<GeoCenter>,
	pub values: TileJsonValues,
	pub vector_layers: VectorLayers,
}

impl TileJSON {
	/// Constructs a `TileJSON` from a [`JsonObject`].
	///
	/// The method looks for special keys: `"bounds"`, `"center"`, and `"vector_layers"`.
	/// All other keys are placed into `self.values`.
	///
	/// # Errors
	///
	/// Returns an error if any required fields are invalid or cannot be converted.
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
					// Convert the "vector_layers" array to a VectorLayers
					r.vector_layers = VectorLayers::from_json_array(v.as_array()?)?;
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

	pub fn as_string(&self) -> String {
		self.as_object().stringify()
	}

	pub fn as_blob(&self) -> Blob {
		Blob::from(self.as_string())
	}

	pub fn update_from_pyramid(&mut self, pyramid: &TileBBoxPyramid) {
		if let Some(bbox) = pyramid.get_geo_bbox() {
			self.limit_bbox(bbox);
		}

		if let Some(z) = pyramid.get_zoom_min() {
			self.limit_min_zoom(z);
		}

		if let Some(z) = pyramid.get_zoom_max() {
			self.limit_min_zoom(z);
		}
	}

	pub fn get_string(&self, key: &str) -> Option<String> {
		self.values.get_string(key)
	}

	pub fn get_str(&self, key: &str) -> Option<&str> {
		self.values.get_str(key)
	}

	pub fn set_byte(&mut self, key: &str, value: u8) -> Result<()> {
		self.values.insert(key, &JsonValue::from(value))
	}

	pub fn set_list(&mut self, key: &str, value: Vec<String>) -> Result<()> {
		self.values.insert(key, &JsonValue::from(value))
	}

	pub fn set_string(&mut self, key: &str, value: &str) -> Result<()> {
		self.values.insert(key, &JsonValue::from(value))
	}

	pub fn set_vector_layers(&mut self, vector_layers: JsonValue) -> Result<()> {
		self.vector_layers = VectorLayers::from_json_array(vector_layers.as_array()?)?;
		Ok(())
	}

	pub fn limit_bbox(&mut self, bbox: GeoBBox) {
		if let Some(ref mut b) = self.bounds {
			b.intersect(&bbox);
		} else {
			self.bounds = Some(bbox);
		}
	}

	pub fn limit_min_zoom(&mut self, z: u8) {
		self.values.update_byte("minzoom", |mz| mz.map_or(z, |mz| mz.max(z)));
	}

	pub fn limit_max_zoom(&mut self, z: u8) {
		self.values.update_byte("maxzoom", |mz| mz.map_or(z, |mz| mz.min(z)));
	}

	/// **Fixed merge method** that merges another `TileJSON` into this one.
	///
	/// - **Bounds:**  
	///   - If `other` has a `bounds`, extends or sets ours accordingly.
	/// - **Center:**  
	///   - If `other.center` is `Some`, it overwrites ours.
	/// - **minzoom/maxzoom:**  
	///   - These are stored in `self.values` under the keys `"minzoom"` and `"maxzoom"`.
	///   - Merges them by taking the min or max respectively.
	/// - **Other `values`:**  
	///   - Merges all other key-value pairs, overwriting if there is a conflict.
	/// - **Vector layers:**  
	///   - Merges all vector layers from `other`. If a layer `id` already exists, it will be overwritten by the `other` one.
	pub fn merge(&mut self, other: &TileJSON) -> Result<()> {
		// 1. Merge bounds: extend or set if we or `other` has them
		if let Some(ob) = &other.bounds {
			self.bounds = match &self.bounds {
				Some(sb) => Some(sb.extended(ob)),
				None => Some(*ob),
			};
		}

		// 2. Overwrite center if `other` has it
		if other.center.is_some() {
			self.center = other.center;
		}

		// 3. Merge minzoom & maxzoom from `values`
		//    By spec, these are bytes (0..=30).
		if let Some(omz) = other.values.get_byte("minzoom") {
			let new_mz = self.values.get_byte("minzoom").map_or(omz, |mz| mz.min(omz));
			self.values.insert("minzoom", &JsonValue::from(new_mz))?;
		}
		if let Some(omz) = other.values.get_byte("maxzoom") {
			let new_mz = self.values.get_byte("maxzoom").map_or(omz, |mz| mz.max(omz));
			self.values.insert("maxzoom", &JsonValue::from(new_mz))?;
		}

		// 4. Merge all other values from `other`, overwriting conflicts.
		//    Exclude "minzoom"/"maxzoom" since we already handled those.
		for (k, v) in other.values.iter_json_values() {
			if k != "minzoom" && k != "maxzoom" {
				let _ = self.values.insert(&k, &v);
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

	/// Performs basic TileJSON checks (common to both raster and vector) based on the spec.
	fn check_basics(&self) -> Result<()> {
		// TileJSON 3.0.0: https://github.com/mapbox/tilejson-spec/tree/master/3.0.0

		// 3.1 tilejson - required
		let version = self
			.values
			.get_string("tilejson")
			.ok_or_else(|| anyhow::anyhow!("Missing tilejson"))?;
		ensure!(
			Regex::new(r"^[123]\.[012]\.[01]$")?.is_match(&version),
			"Invalid tilejson version"
		);

		// 3.2 tiles - optional
		self.values.check_optional_list("tiles")?;

		// 3.3 vector_layers - is checked in check_vector() or check_raster()

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
	/// - Must have at least one `vector_layer`.
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
