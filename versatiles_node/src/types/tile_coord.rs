use crate::napi_result;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use versatiles_core::TileCoord as RustTileCoord;

/// Tile coordinate with zoom level (z), column (x), and row (y)
#[napi]
pub struct TileCoord {
	inner: RustTileCoord,
}

#[napi]
impl TileCoord {
	/// Create a new TileCoord
	#[napi(constructor)]
	pub fn new(z: u32, x: u32, y: u32) -> Result<Self> {
		let inner = napi_result!(RustTileCoord::new(z as u8, x, y))?;
		Ok(Self { inner })
	}

	/// Create a TileCoord from geographic coordinates
	#[napi(factory)]
	pub fn from_geo(lon: f64, lat: f64, z: u32) -> Result<Self> {
		let inner = napi_result!(RustTileCoord::from_geo(lon, lat, z as u8))?;
		Ok(Self { inner })
	}

	/// Convert to geographic coordinates [longitude, latitude]
	#[napi]
	pub fn to_geo(&self) -> Vec<f64> {
		let [lon, lat] = self.inner.as_geo();
		vec![lon, lat]
	}

	/// Get the geographic bounding box [west, south, east, north]
	#[napi]
	pub fn to_geo_bbox(&self) -> Vec<f64> {
		self.inner.to_geo_bbox().as_array().to_vec()
	}

	/// Get the zoom level
	#[napi(getter)]
	pub fn z(&self) -> u32 {
		self.inner.level as u32
	}

	/// Get the column (x)
	#[napi(getter)]
	pub fn x(&self) -> u32 {
		self.inner.x
	}

	/// Get the row (y)
	#[napi(getter)]
	pub fn y(&self) -> u32 {
		self.inner.y
	}

	/// Get JSON representation
	#[napi]
	pub fn to_json(&self) -> String {
		self.inner.as_json()
	}
}
