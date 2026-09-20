//! Decode arbitrary bytes as a Mapbox Vector Tile.
//!
//! The protobuf reader is hand-written rather than generated, so its bounds and
//! arithmetic are this project's to get right. An eleven-byte tile whose
//! layer-name length claimed 2^50 asked the allocator for a petabyte and
//! aborted the process — a shape a fuzzer reaches in seconds.

#![no_main]

use libfuzzer_sys::fuzz_target;
use versatiles_core::Blob;
use versatiles_geometry::vector_tile::VectorTile;

fuzz_target!(|data: &[u8]| {
	// Errors are the expected outcome for almost every input. What this looks
	// for is a panic, an abort, or a hang.
	if let Ok(tile) = VectorTile::from_blob(&Blob::from(data.to_vec())) {
		// Re-encoding walks every layer, feature and property, so it reaches
		// the parts of the tree that merely decoding leaves untouched.
		let _ = tile.to_blob();
	}
});
