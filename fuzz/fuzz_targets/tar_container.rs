//! Open arbitrary bytes as a tar of tiles and read from it.
//!
//! The reader indexes every entry when it opens, so the headers' sizes,
//! names and extensions are all parsed before any tile is asked for. It reads
//! from a file, so each input is written to one first.

#![no_main]

use std::{path::PathBuf, sync::LazyLock};

use libfuzzer_sys::fuzz_target;
use versatiles_container::{TarTilesReader, TileSource, TilesRuntime};
use versatiles_core::TileCoord;

/// One runtime for the whole run: building one per input would measure tokio
/// rather than the reader.
static RUNTIME: LazyLock<tokio::runtime::Runtime> =
	LazyLock::new(|| tokio::runtime::Builder::new_current_thread().build().unwrap());

/// One file per process, overwritten for every input. The process id keeps
/// parallel fuzzing jobs (`-jobs`, `-workers`) off each other's file.
static PATH: LazyLock<PathBuf> =
	LazyLock::new(|| std::env::temp_dir().join(format!("versatiles-fuzz-{}.tar", std::process::id())));

fuzz_target!(|data: &[u8]| {
	if std::fs::write(&*PATH, data).is_err() {
		return;
	}
	RUNTIME.block_on(async {
		let Ok(container) = TarTilesReader::open(&PATH, TilesRuntime::default()).await else {
			return;
		};
		let _ = container.tile_pyramid().await;
		for coord in [(0, 0, 0), (14, 8800, 5370), (30, 0, 0)] {
			if let Ok(coord) = TileCoord::new(coord.0, coord.1, coord.2) {
				let _ = container.tile(&coord).await;
			}
		}
	});
});
