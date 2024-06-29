//! Types used for handling tile streams and probing depths in tile containers.
//!
//! This module provides types and utilities for managing tile data streams and probing the depth of tile containers.

/// Enum representing the depth of probing for a tile container.
pub enum ProbeDepth {
	/// Shallow probing depth.
	Shallow = 0,
	/// Container level probing depth.
	Container = 1,
	/// Tiles level probing depth.
	Tiles = 2,
	/// Tile contents level probing depth.
	TileContents = 3,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_probe_depth() {
		probe_container(ProbeDepth::Shallow);
		probe_container(ProbeDepth::Container);
		probe_container(ProbeDepth::Tiles);
		probe_container(ProbeDepth::TileContents);
	}

	fn probe_container(depth: ProbeDepth) {
		match depth {
			ProbeDepth::Shallow => println!("Performing a shallow probe"),
			ProbeDepth::Container => println!("Probing container level"),
			ProbeDepth::Tiles => println!("Probing tiles level"),
			ProbeDepth::TileContents => println!("Probing tile contents level"),
		}
	}
}
