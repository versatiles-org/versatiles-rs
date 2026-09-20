//! Open arbitrary bytes as a PMTiles container and read from it.
//!
//! Three of the audit's findings live on this path and all three are shallow
//! enough for a fuzzer: a leaf directory entry pointing at its own bytes
//! (unbounded recursion, then SIGABRT), a `run_length` of 2^32 costing one
//! varint byte, and an entry-count varint no directory could back.

#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use versatiles_container::{PMTilesReader, TileSource, TilesRuntime};
use versatiles_core::{Blob, TileCoord, io::DataReaderBlob};

/// One runtime for the whole run: building one per input would measure tokio
/// rather than the parser.
static RUNTIME: LazyLock<tokio::runtime::Runtime> =
	LazyLock::new(|| tokio::runtime::Builder::new_current_thread().build().unwrap());

fuzz_target!(|data: &[u8]| {
	RUNTIME.block_on(async {
		let reader = DataReaderBlob::from(Blob::from(data.to_vec()));
		let Ok(container) = PMTilesReader::open_data(Box::new(reader), TilesRuntime::default()).await else {
			return;
		};

		// Walking the directory tree is a different descent from looking one
		// tile up: the lookup path has always been depth-capped, the pyramid
		// walk was not.
		let _ = container.tile_pyramid().await;

		// A lookup exercises entry search and the byte-range arithmetic that
		// turns an entry into a read.
		for coord in [(0, 0, 0), (14, 8800, 5370), (30, 0, 0)] {
			if let Ok(coord) = TileCoord::new(coord.0, coord.1, coord.2) {
				let _ = container.tile(&coord).await;
			}
		}
	});
});
