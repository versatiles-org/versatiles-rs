//! Open arbitrary bytes as a `.versatiles` container and read from it.
//!
//! The block index, the per-block tile index and the tile ranges are three
//! separate structures that a file can make disagree with each other. The
//! audit's example was a 105-byte file whose block claimed 65,536 tiles while
//! its index held one, which indexed out of bounds on an ordinary tile request.

#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use versatiles_container::{TileSource, TilesRuntime, VersaTilesReader};
use versatiles_core::{Blob, TileCoord, io::DataReaderBlob};

/// One runtime for the whole run; see the PMTiles target.
static RUNTIME: LazyLock<tokio::runtime::Runtime> =
	LazyLock::new(|| tokio::runtime::Builder::new_current_thread().build().unwrap());

fuzz_target!(|data: &[u8]| {
	RUNTIME.block_on(async {
		let reader = DataReaderBlob::from(Blob::from(data.to_vec()));
		let Ok(container) = VersaTilesReader::open_data(Box::new(reader), TilesRuntime::default()).await else {
			return;
		};

		let _ = container.tile_pyramid().await;

		// Coordinates inside a declared block but not necessarily inside its
		// index: that disagreement is the interesting one here.
		for coord in [(0, 0, 0), (8, 5, 5), (14, 8800, 5370)] {
			if let Ok(coord) = TileCoord::new(coord.0, coord.1, coord.2) {
				let _ = container.tile(&coord).await;
			}
		}
	});
});
