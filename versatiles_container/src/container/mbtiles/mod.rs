//! `SQLite` file `*.mbtiles` as tile container
//!
//! This module provides structures and implementations for reading and writing tiles to and from an `MBTiles` `SQLite` database.
//!
//! The main components of this module are:
//! - `MBTilesReader`: Reads tiles from an `MBTiles` `SQLite` database.
//! - `MBTilesWriter`: Writes tiles to an `MBTiles` `SQLite` database.
//!
//! ## Arithmetic
//! Rows are flipped between XYZ and TMS with
//! [`TileCoord::flip_y`](versatiles_core::TileCoord::flip_y), not by hand, and
//! the lint keeps it that way (#294). Tests are exempt.
#![cfg_attr(not(test), deny(clippy::arithmetic_side_effects))]

mod reader;
mod sink;
mod writer;

use std::{
	path::{Path, PathBuf},
	sync::{Arc, LazyLock},
};

use anyhow::{Result, bail};
use r2d2::Pool;
pub use reader::MBTilesReader;
use rusqlite::{Connection, OpenFlags};
use scheduled_thread_pool::ScheduledThreadPool;
pub use sink::MBTilesTileSink;
use versatiles_core::{TileCompression, TileFormat};
pub use writer::MBTilesWriter;

/// The scheduler that every `SQLite` connection pool in this module runs its
/// background jobs on.
///
/// Without one, r2d2 starts three threads for each pool, and they outlive the
/// pool by up to 30 seconds while they wait for its next reaper run. Opening
/// files in quick succession, as a pipeline over many sources or a fuzzer
/// does, piled them up until the process could start no more threads. The
/// jobs are short and rare, so one scheduler serves every pool.
static POOL_SCHEDULER: LazyLock<Arc<ScheduledThreadPool>> = LazyLock::new(|| {
	Arc::new(
		ScheduledThreadPool::builder()
			.num_threads(3)
			.thread_name_pattern("versatiles-mbtiles-{}")
			.build(),
	)
});

/// Opens the `SQLite` connections of a pool, all to one file and with the same
/// flags.
///
/// What `r2d2_sqlite` provided, and all of it this module used. That crate
/// lags `rusqlite` releases, and its 0.35 held `rusqlite` at 0.40, whose
/// bundled `SQLite` has RUSTSEC-2026-0459: a crafted `-wal` file next to an
/// `MBTiles` file made queries return stale process memory, which `serve`
/// would have sent out as tiles. With this, `rusqlite` updates on its own.
#[derive(Debug)]
pub(crate) struct SqliteConnectionManager {
	path: PathBuf,
	flags: OpenFlags,
}

impl SqliteConnectionManager {
	/// Connections to the file at `path`, read-write and created if missing,
	/// as `rusqlite` opens a file by default.
	pub(crate) fn file(path: impl AsRef<Path>) -> Self {
		Self {
			path: path.as_ref().to_path_buf(),
			flags: OpenFlags::default(),
		}
	}

	/// The same connections, opened with `flags` instead.
	pub(crate) fn with_flags(self, flags: OpenFlags) -> Self {
		Self { flags, ..self }
	}
}

impl r2d2::ManageConnection for SqliteConnectionManager {
	type Connection = Connection;
	type Error = rusqlite::Error;

	fn connect(&self) -> rusqlite::Result<Connection> {
		Connection::open_with_flags(&self.path, self.flags)
	}

	/// The check r2d2 runs on every checkout: an empty batch, which fails
	/// only if the connection is unusable.
	fn is_valid(&self, connection: &mut Connection) -> rusqlite::Result<()> {
		connection.execute_batch("")
	}

	fn has_broken(&self, _connection: &mut Connection) -> bool {
		false
	}
}

/// Build the connection pool for one `MBTiles` file, on the shared scheduler.
pub(crate) fn build_pool(manager: SqliteConnectionManager) -> Result<Pool<SqliteConnectionManager>> {
	Ok(Pool::builder()
		.max_size(10)
		.thread_pool(Arc::clone(&POOL_SCHEDULER))
		.build(manager)?)
}

/// How MBTiles stores a given tile format: the value for its `format` metadata
/// row, and the one compression it accepts.
///
/// MBTiles allows exactly one compression per format — raster formats
/// uncompressed, vector tiles gzipped — so this is a mapping rather than a
/// check. MLT, a format the spec predates, is written under its media type as
/// the spec asks for such formats, and uncompressed, since it compresses
/// internally; the reader also takes the gzipped MLT tiles other tools write. Shared by the writer, which recompresses to match, and the sink,
/// which only receives finished blobs and can therefore just verify.
pub(crate) fn required_encoding(format: TileFormat) -> Result<(&'static str, TileCompression)> {
	use TileCompression::{Gzip, Uncompressed};
	use TileFormat::{JPG, MLT, MVT, PNG, WEBP};

	Ok(match format {
		JPG => ("jpg", Uncompressed),
		MVT => ("pbf", Gzip),
		MLT => ("application/vnd.maplibre-tile", Uncompressed),
		PNG => ("png", Uncompressed),
		WEBP => ("webp", Uncompressed),
		other => bail!("MBTiles cannot store {other} tiles; it supports jpg, png, webp, pbf (mvt) and mlt"),
	})
}
