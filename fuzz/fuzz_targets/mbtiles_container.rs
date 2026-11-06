//! Open arbitrary bytes as an MBTiles file and read from it.
//!
//! SQLite is bundled C code, and SQLite fuzzes itself. What this target is for
//! is the reader on top: an `.mbtiles` file is untrusted input, so its metadata
//! table, its zoom levels and its tile blobs may say anything. SQLite only
//! opens files, so each input is written to a file first, which makes this
//! target slower than the others.

#![no_main]

use std::{path::PathBuf, sync::LazyLock};

use libfuzzer_sys::fuzz_target;
use versatiles_container::{MBTilesReader, TileSource, TilesRuntime};
use versatiles_core::TileCoord;

/// One runtime for the whole run: building one per input would measure tokio
/// rather than the reader.
static RUNTIME: LazyLock<tokio::runtime::Runtime> =
	LazyLock::new(|| tokio::runtime::Builder::new_current_thread().build().unwrap());

/// One file per process, overwritten for every input. The process id keeps
/// parallel fuzzing jobs (`-jobs`, `-workers`) off each other's file.
static PATH: LazyLock<PathBuf> =
	LazyLock::new(|| std::env::temp_dir().join(format!("versatiles-fuzz-{}.mbtiles", std::process::id())));

fuzz_target!(|data: &[u8]| {
	if std::fs::write(&*PATH, data).is_err() {
		return;
	}
	let Ok(container) = MBTilesReader::open(&PATH, TilesRuntime::default()) else {
		return;
	};
	RUNTIME.block_on(async {
		let _ = container.tile_pyramid().await;
		for coord in [(0, 0, 0), (14, 8800, 5370), (30, 0, 0)] {
			if let Ok(coord) = TileCoord::new(coord.0, coord.1, coord.2) {
				let _ = container.tile(&coord).await;
			}
		}
	});
});
