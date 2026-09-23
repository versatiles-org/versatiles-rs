//! This module provides functionality for writing data to files.
//!
//! # Overview
//!
//! The `DataWriterFile` struct allows for writing data to files on the filesystem.
//! It implements the `DataWriterTrait` to provide methods for appending data, writing data from the start,
//! and managing the write position. The module ensures the file path is absolute before attempting to create or write to the file.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::{io::{DataWriterFile, DataWriterTrait}, Blob, ByteRange};
//! use anyhow::Result;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let path = std::env::temp_dir().join("temp1.txt");
//!     let mut writer = DataWriterFile::from_path(&path)?;
//!     let data = Blob::from(vec![1, 2, 3, 4]);
//!
//!     // Appending data
//!     writer.append(&data).await?;
//!     assert_eq!(writer.position().await?, 4);
//!
//!     // Writing data from the start
//!     writer.write_start(&Blob::from(vec![5, 6, 7, 8])).await?;
//!     writer.set_position(0).await?;
//!     assert_eq!(writer.position().await?, 0);
//!
//!     Ok(())
//! }
//! ```

use std::{
	fs::File,
	io::{BufWriter, Seek, SeekFrom, Write},
	path::Path,
};

use anyhow::{Result, ensure};
use async_trait::async_trait;
use versatiles_derive::context;

use super::DataWriterTrait;
use crate::{Blob, ByteRange};

/// A struct that provides writing capabilities to a file.
pub struct DataWriterFile {
	writer: BufWriter<File>,
}

// The `DataWriterTrait` methods below are `async` because the trait is — SFTP
// needs it — but their bodies deliberately use blocking `std::fs`. Two reasons,
// both of which make `tokio::fs` a worse fit here rather than a better one:
//
//  - `tokio::fs` is `spawn_blocking` underneath on every platform, so it does
//    not remove the blocking; it moves it to a pool thread and adds a
//    round-trip per operation. A buffered local write is short and never
//    yields, so paying that per call buys nothing.
//  - `std::io::BufWriter` flushes on `Drop`; `tokio::io::BufWriter` cannot,
//    because a destructor cannot await. Every path that builds a
//    `DataWriterFile` now calls `finalize()`, so that destructor is a backstop
//    rather than the mechanism — but it is the backstop that keeps a missed
//    call from truncating a file outright, and switching would remove it.
//
// A further trap if anyone does revisit this: `append` and `write_start` call
// `stream_position`, which `std` specialises on `BufWriter` to read the
// position without flushing. Tokio's `BufWriter` flushes on every seek, so a
// direct port would flush the buffer on each append. Track the position
// manually instead, the way `DataWriterSftp` does.

impl DataWriterFile {
	/// Creates a `DataWriterFile` from a file path.
	///
	/// # Arguments
	///
	/// * `path` - A reference to the file path to create and write to.
	///
	/// # Returns
	///
	/// * A Result containing the new `DataWriterFile` instance or an error.
	#[context("while creating file writer for path {:?}", path)]
	pub fn from_path(path: &Path) -> Result<DataWriterFile> {
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		Ok(DataWriterFile {
			writer: BufWriter::new(File::create(path)?),
		})
	}
}

#[async_trait]
#[async_trait]
impl DataWriterTrait for DataWriterFile {
	/// Appends data to the file.
	///
	/// # Arguments
	///
	/// * `blob` - A reference to the `Blob` to append.
	///
	/// # Returns
	///
	/// * A Result containing a `ByteRange` indicating the position and length of the appended data, or an error.
	#[context("while appending {} bytes to file", blob.len())]
	async fn append(&mut self, blob: &Blob) -> Result<ByteRange> {
		let pos = self.writer.stream_position()?;
		// `write_all`, not `write`: a blob larger than the 8 KiB buffer is passed
		// straight to the file, where a single `write(2)` may be short. `write`
		// would then return a `ByteRange` shorter than the blob and report
		// success — an index entry pointing at bytes that were never written.
		self.writer.write_all(blob.as_slice())?;

		Ok(ByteRange::new(pos, blob.len()))
	}

	/// Writes data from the start of the file.
	///
	/// # Arguments
	///
	/// * `blob` - A reference to the `Blob` to write.
	///
	/// # Returns
	///
	/// * A Result indicating success or an error.
	#[context("while writing {} bytes at start of file", blob.len())]
	async fn write_start(&mut self, blob: &Blob) -> Result<()> {
		let pos = self.writer.stream_position()?;
		self.writer.rewind()?;
		self.writer.write_all(blob.as_slice())?;
		self.writer.seek(SeekFrom::Start(pos))?;
		Ok(())
	}

	/// Gets the current write position.
	///
	/// # Returns
	///
	/// * A Result containing the current write position in bytes or an error.
	#[context("while getting current write position")]
	async fn position(&mut self) -> Result<u64> {
		Ok(self.writer.stream_position()?)
	}

	/// Sets the write position.
	///
	/// # Arguments
	///
	/// * `position` - The position to set in bytes.
	///
	/// # Returns
	///
	/// * A Result indicating success or an error.
	#[context("while setting write position to {}", position)]
	async fn set_position(&mut self, position: u64) -> Result<()> {
		self.writer.seek(SeekFrom::Start(position))?;
		Ok(())
	}

	/// Flushes the buffer, so a failure to write the last bytes is an error
	/// rather than a silently truncated file.
	///
	/// Without this, the trait's promise — "after `finalize` returns `Ok`, the
	/// destination holds all written bytes" — was false here: the default
	/// `finalize` is a no-op, so the buffer reached the disk only through
	/// `BufWriter`'s destructor, which cannot return an error and discards the
	/// one it gets.
	///
	/// **This is a latent hole, not a live bug.** Every writer today ends with
	/// `write_start`, and `BufWriter`'s `Seek` flushes, so by the time `finalize`
	/// is called the buffer is empty — measured at 0 bytes for both the
	/// `.versatiles` and `.pmtiles` routes. A writer whose last operation is an
	/// `append` would be the one to lose its tail, and nothing in the type system
	/// stops someone writing that. The destructor stays as the backstop.
	///
	/// Flush, not `sync_all`: this reports what the filesystem has accepted,
	/// which is what surfaces `ENOSPC` and a failing disk. Durability across a
	/// power cut is a separate question with a real cost per conversion, and is
	/// not decided here.
	#[context("while flushing buffered data to file")]
	async fn finalize(&mut self) -> Result<()> {
		self.writer.flush()?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use std::{fs::File, io::Read};

	use anyhow::Result;
	use assert_fs::NamedTempFile;

	use super::*;
	use crate::Blob;

	/// The trait promises that after `finalize` returns `Ok` the destination
	/// holds all written bytes. Read the file through a *separate* handle while
	/// the writer is still alive: with `finalize` a no-op, the bytes are still
	/// sitting in the `BufWriter` and the file on disk is empty.
	#[tokio::test]
	async fn finalize_puts_the_bytes_on_disk_before_the_writer_is_dropped() -> Result<()> {
		let temp = NamedTempFile::new("finalize")?;
		let mut writer = DataWriterFile::from_path(temp.path())?;
		writer.append(&Blob::from(vec![1, 2, 3, 4, 5])).await?;

		writer.finalize().await?;

		let on_disk = std::fs::read(temp.path())?;
		assert_eq!(on_disk, vec![1, 2, 3, 4, 5], "finalize must leave nothing buffered");
		Ok(())
	}

	/// Pins the contract a container index depends on: the returned `ByteRange`
	/// covers the whole blob, and the whole blob reaches the file.
	///
	/// A blob this size bypasses `BufWriter`'s 8 KiB buffer and reaches
	/// `write(2)` directly, where a short write is legal — `write` would then
	/// return a range shorter than the blob and report success, indexing bytes
	/// that are not there, which is why `append` uses `write_all`. This test does
	/// **not** reproduce that: a regular file on a normal filesystem does not
	/// write short at this size, so it passes either way. It guards the
	/// arithmetic, not the syscall.
	#[tokio::test]
	async fn a_blob_larger_than_the_buffer_is_written_whole() -> Result<()> {
		let temp = NamedTempFile::new("big")?;
		let mut writer = DataWriterFile::from_path(temp.path())?;

		let big = Blob::from(vec![7u8; 100_000]);
		let range = writer.append(&big).await?;
		writer.finalize().await?;

		assert_eq!(range.length, 100_000, "the reported range must cover the whole blob");
		assert_eq!(range.offset, 0);
		let on_disk = std::fs::read(temp.path())?;
		assert_eq!(on_disk.len(), 100_000, "every byte must reach the file");
		assert!(on_disk.iter().all(|&b| b == 7));
		Ok(())
	}

	/// Two large appends in a row: the second range must start exactly where the
	/// first ended, which is what the container indexes rely on.
	#[tokio::test]
	async fn consecutive_large_appends_stay_contiguous() -> Result<()> {
		let temp = NamedTempFile::new("big2")?;
		let mut writer = DataWriterFile::from_path(temp.path())?;

		let first = writer.append(&Blob::from(vec![1u8; 50_000])).await?;
		let second = writer.append(&Blob::from(vec![2u8; 50_000])).await?;
		writer.finalize().await?;

		assert_eq!(first.offset, 0);
		assert_eq!(second.offset, first.length, "no gap and no overlap");
		assert_eq!(std::fs::read(temp.path())?.len(), 100_000);
		Ok(())
	}

	#[tokio::test]
	async fn test_append_and_position() -> Result<()> {
		// Create a temporary file
		let temp = NamedTempFile::new("test1")?;
		let path = temp.path();
		// Ensure absolute path
		assert!(path.is_absolute());

		let mut writer = DataWriterFile::from_path(path)?;
		let data = Blob::from(vec![10, 20, 30]);
		// Append data
		let range = writer.append(&data).await?;
		assert_eq!(range.to_string(), "[0..=2]");
		// Position should now equal length
		assert_eq!(writer.position().await?, 3);

		// Read back file contents
		let mut file = File::open(path)?;
		let mut buf = Vec::new();
		file.read_to_end(&mut buf)?;
		assert_eq!(buf, data.as_slice());
		Ok(())
	}

	#[tokio::test]
	async fn test_write_start_and_append() -> Result<()> {
		let temp = NamedTempFile::new("test2")?;
		let path = temp.path();
		let mut writer = DataWriterFile::from_path(path)?;

		// Write start should write data at position 0
		let start = Blob::from(vec![1, 2, 3, 4]);
		writer.write_start(&start).await?;
		// After write_start, position unchanged (0)
		assert_eq!(writer.position().await?, 0);

		// Now append more data
		let extra = Blob::from(vec![5, 6]);
		let range2 = writer.append(&extra).await?;
		assert_eq!(range2.to_string(), "[0..=1]");

		drop(writer);

		// Read back file contents
		let mut file = File::open(path)?;
		let mut buf = Vec::new();
		file.read_to_end(&mut buf)?;
		// File should contain extra data because append writes at offset 0 after write_start
		assert_eq!(buf, &[5, 6, 3, 4]);
		Ok(())
	}
}
