//! Types used for handling tile streams and probing depths in tile containers.
//!
//! This module provides types and utilities for defining the depth level of probing
//! operations on tile data. Depending on the probe depth, certain levels of the tile
//! container (container metadata, tile entries, tile contents) may be interrogated.

/// An enum representing the depth of probing for a tile container.
///
/// Each variant indicates how deeply to analyze tile data. Higher-depth variants are
/// supersets of shallower ones, meaning that if you probe at a "Tiles" level,
/// you might retrieve more detailed information than a "Shallow" or "Container" probe.
///
/// # Usage
///
/// ```rust
/// use versatiles_core::ProbeDepth;
///
/// fn probe(depth: ProbeDepth) {
///     match depth {
///         ProbeDepth::Shallow => println!("Performing a shallow probe"),
///         ProbeDepth::Container => println!("Probing container-level metadata"),
///         ProbeDepth::Tiles => println!("Probing each tile's metadata"),
///         ProbeDepth::TileContents => println!("Probing tile contents in detail"),
///     }
/// }
///
/// probe(ProbeDepth::Shallow);
/// probe(ProbeDepth::TileContents);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeDepth {
	/// Shallow probing depth:
	/// Only gather minimal metadata about the container or its format.
	Shallow = 0,

	/// Container-level probing depth:
	/// Retrieve container-wide metadata or summary, but do not iterate tiles.
	Container = 1,

	/// Tiles-level probing depth:
	/// Inspect each tile's basic metadata (e.g., tile count, bounding boxes),
	/// but do not examine tile contents.
	Tiles = 2,

	/// Tile contents-level probing depth:
	/// Fully read the tile data, including its uncompressed or detailed structure.
	TileContents = 3,
}

#[cfg(test)]
mod tests {
	use super::ProbeDepth;

	/// A helper function that simulates how a system might behave differently
	/// depending on the probing depth.
	fn simulate_probe(depth: ProbeDepth) -> String {
		match depth {
			ProbeDepth::Shallow => "Shallow probe: Minimal info gathered.".into(),
			ProbeDepth::Container => "Container probe: Container-level metadata gathered.".into(),
			ProbeDepth::Tiles => "Tiles probe: Tile-level metadata gathered.".into(),
			ProbeDepth::TileContents => "TileContents probe: Full tile contents gathered.".into(),
		}
	}

	/// Simple test that ensures we can call each variant and get the expected output.
	#[test]
	fn test_all_depth_variants() {
		assert_eq!(
			simulate_probe(ProbeDepth::Shallow),
			"Shallow probe: Minimal info gathered."
		);
		assert_eq!(
			simulate_probe(ProbeDepth::Container),
			"Container probe: Container-level metadata gathered."
		);
		assert_eq!(
			simulate_probe(ProbeDepth::Tiles),
			"Tiles probe: Tile-level metadata gathered."
		);
		assert_eq!(
			simulate_probe(ProbeDepth::TileContents),
			"TileContents probe: Full tile contents gathered."
		);
	}

	/// Checks that the enum discriminants match their documented values.
	/// This can be important if you're serializing the numeric values.
	#[test]
	fn test_discriminants() {
		assert_eq!(ProbeDepth::Shallow as i32, 0);
		assert_eq!(ProbeDepth::Container as i32, 1);
		assert_eq!(ProbeDepth::Tiles as i32, 2);
		assert_eq!(ProbeDepth::TileContents as i32, 3);
	}
}
