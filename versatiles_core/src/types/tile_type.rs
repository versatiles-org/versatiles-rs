/// Represents the type of a tile, which can be either `Raster`, `Vector`, or `Unknown`.
use anyhow::bail;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileType {
	/// Represents raster tile type.
	Raster,
	/// Represents vector tile type.
	Vector,
	/// Represents an unknown tile type.
	Unknown,
}

impl TileType {
	/// Returns the string representation of the tile type.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileType;
	///
	/// assert_eq!(TileType::Raster.as_str(), "raster");
	/// assert_eq!(TileType::Vector.as_str(), "vector");
	/// assert_eq!(TileType::Unknown.as_str(), "unknown");
	/// ```
	#[must_use] 
	pub fn as_str(&self) -> &str {
		use TileType::*;
		match self {
			Raster => "raster",
			Vector => "vector",
			Unknown => "unknown",
		}
	}

	#[must_use] 
	pub fn is_raster(&self) -> bool {
		*self == TileType::Raster
	}

	#[must_use] 
	pub fn is_vector(&self) -> bool {
		*self == TileType::Vector
	}

	/// Returns the default tile schema associated with the tile type.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileType;
	///
	/// assert_eq!(TileType::Raster.get_default_tile_schema(), Some("rgb"));
	/// assert_eq!(TileType::Vector.get_default_tile_schema(), Some("other"));
	/// assert_eq!(TileType::Unknown.get_default_tile_schema(), None);
	/// ```
	#[must_use] 
	pub fn get_default_tile_schema(&self) -> Option<&'static str> {
		use TileType::*;
		match self {
			Raster => Some("rgb"),
			Vector => Some("other"),
			Unknown => None,
		}
	}
}

impl std::fmt::Display for TileType {
	/// Formats the tile type as a string for display purposes.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileType;
	///
	/// assert_eq!(format!("{}", TileType::Raster), "raster");
	/// assert_eq!(format!("{}", TileType::Vector), "vector");
	/// assert_eq!(format!("{}", TileType::Unknown), "unknown");
	/// ```
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.as_str())
	}
}

impl TryFrom<&str> for TileType {
	type Error = anyhow::Error;

	/// Attempts to convert a string into a `TileType`.
	///
	/// # Arguments
	///
	/// * `value` - A string slice representing the tile type.
	///
	/// # Returns
	///
	/// * `Ok(TileType)` if the string matches a valid tile type.
	/// * `Err(anyhow::Error)` if the string does not match any valid tile type.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileType;
	/// use std::convert::TryFrom;
	///
	/// assert_eq!(TileType::try_from("raster").unwrap(), TileType::Raster);
	/// assert_eq!(TileType::try_from("image").unwrap(), TileType::Raster);
	/// assert_eq!(TileType::try_from("vector").unwrap(), TileType::Vector);
	/// assert_eq!(TileType::try_from("unknown").unwrap(), TileType::Unknown);
	/// assert!(TileType::try_from("invalid").is_err());
	/// assert!(TileType::try_from("").is_err());
	/// ```
	fn try_from(value: &str) -> Result<Self, Self::Error> {
		match value {
			"image" | "raster" => Ok(TileType::Raster),
			"vector" => Ok(TileType::Vector),
			"unknown" => Ok(TileType::Unknown),
			_ => bail!("Invalid tile content type: {value}"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_as_str() {
		assert_eq!(TileType::Raster.as_str(), "raster");
		assert_eq!(TileType::Vector.as_str(), "vector");
		assert_eq!(TileType::Unknown.as_str(), "unknown");
	}

	#[test]
	fn test_get_default_tile_schema() {
		assert_eq!(TileType::Raster.get_default_tile_schema(), Some("rgb"));
		assert_eq!(TileType::Vector.get_default_tile_schema(), Some("other"));
		assert_eq!(TileType::Unknown.get_default_tile_schema(), None);
	}

	#[test]
	fn test_display() {
		assert_eq!(format!("{}", TileType::Raster), "raster");
		assert_eq!(format!("{}", TileType::Vector), "vector");
		assert_eq!(format!("{}", TileType::Unknown), "unknown");
	}

	#[test]
	fn test_try_from_str_valid() {
		assert_eq!(TileType::try_from("raster").unwrap(), TileType::Raster);
		assert_eq!(TileType::try_from("image").unwrap(), TileType::Raster);
		assert_eq!(TileType::try_from("vector").unwrap(), TileType::Vector);
		assert_eq!(TileType::try_from("unknown").unwrap(), TileType::Unknown);
	}

	#[test]
	fn test_try_from_str_invalid() {
		assert!(TileType::try_from("invalid").is_err());
		assert!(TileType::try_from("").is_err());
	}
}
