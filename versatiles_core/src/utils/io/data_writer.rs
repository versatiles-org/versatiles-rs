//! This module defines the `DataWriterTrait` trait for writing data to various destinations.
//!
//! # Overview
//!
//! The `DataWriterTrait` trait provides an interface for writing data to different destinations.
//! Implementations of this trait can append data, write data from the start, and manage the write position.
//! This trait is designed to be implemented by any struct that handles data writing operations.
//!
//! # Examples
//!
//! ```rust
//! use versatiles::{utils::io::DataWriterTrait, types::{Blob, ByteRange}};
//! use anyhow::Result;
//!
//! struct MockDataWriter {
//!     data: Vec<u8>,
//!     position: u64,
//! }
//!
//! impl DataWriterTrait for MockDataWriter {
//!     fn append(&mut self, blob: &Blob) -> Result<ByteRange> {
//!         let pos = self.position;
//!         self.data.extend_from_slice(blob.as_slice());
//!         self.position += blob.len() as u64;
//!         Ok(ByteRange::new(pos, blob.len() as u64))
//!     }
//!
//!     fn write_start(&mut self, blob: &Blob) -> Result<()> {
//!         self.data.splice(0..blob.len() as usize, blob.as_slice().iter().cloned());
//!         Ok(())
//!     }
//!
//!     fn get_position(&mut self) -> Result<u64> {
//!         Ok(self.position)
//!     }
//!
//!     fn set_position(&mut self, position: u64) -> Result<()> {
//!         self.position = position;
//!         Ok(())
//!     }
//! }
//!
//! fn main() -> Result<()> {
//!     let mut writer = MockDataWriter { data: vec![], position: 0 };
//!     let data = Blob::from(vec![1, 2, 3, 4]);
//!
//!     // Appending data
//!     let range = writer.append(&data)?;
//!     assert_eq!(range, ByteRange::new(0, 4));
//!
//!     // Writing data from the start
//!     writer.write_start(&Blob::from(vec![5, 6, 7, 8]))?;
//!     assert_eq!(writer.data, vec![5, 6, 7, 8]);
//!
//!     Ok(())
//! }
//! ```

use crate::types::{Blob, ByteRange};
use anyhow::Result;

/// A trait for writing data to various destinations.
///
/// # Required Methods
/// - `append`: Appends data to the writer.
/// - `write_start`: Writes data from the start of the writer.
/// - `get_position`: Gets the current write position.
/// - `set_position`: Sets the write position.
pub trait DataWriterTrait: Send {
	/// Appends data to the writer.
	///
	/// # Arguments
	///
	/// * `blob` - A reference to the `Blob` to append.
	///
	/// # Returns
	///
	/// * A Result containing a `ByteRange` indicating the position and length of the appended data, or an error.
	fn append(&mut self, blob: &Blob) -> Result<ByteRange>;

	/// Writes data from the start of the writer.
	///
	/// # Arguments
	///
	/// * `blob` - A reference to the `Blob` to write.
	///
	/// # Returns
	///
	/// * A Result indicating success or an error.
	fn write_start(&mut self, blob: &Blob) -> Result<()>;

	/// Gets the current write position.
	///
	/// # Returns
	///
	/// * A Result containing the current write position in bytes or an error.
	fn get_position(&mut self) -> Result<u64>;

	/// Sets the write position.
	///
	/// # Arguments
	///
	/// * `position` - The position to set in bytes.
	///
	/// # Returns
	///
	/// * A Result indicating success or an error.
	fn set_position(&mut self, position: u64) -> Result<()>;
}
