//! This module defines the `DataReaderTrait` and associated types for reading data from various sources.
//!
//! # Overview
//!
//! The `DataReaderTrait` trait provides an interface for reading data from different sources. Implementations
//! of this trait can read specific ranges of bytes or all the data from the source. This module also defines
//! the `DataReader` type alias for a boxed dynamic implementation of the trait.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::{io::{DataReaderTrait, DataReader}, Blob, ByteRange};
//! use anyhow::Result;
//! use async_trait::async_trait;
//!
//! #[derive(Debug)]
//! struct MockDataReader {
//!     data: Vec<u8>,
//! }
//!
//! #[async_trait]
//! impl DataReaderTrait for MockDataReader {
//!     async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
//!         let end = (range.offset + range.length) as usize;
//!         let data_slice = &self.data[range.offset as usize..end];
//!         Ok(Blob::from(data_slice.to_vec()))
//!     }
//!
//!     async fn read_all(&self) -> Result<Blob> {
//!         Ok(Blob::from(self.data.clone()))
//!     }
//!
//!     fn get_name(&self) -> &str {
//!         "MockDataReader"
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let data = vec![1, 2, 3, 4, 5];
//!     let mut reader: DataReader = Box::new(MockDataReader { data });
//!
//!     // Reading a range of data
//!     let range = ByteRange { offset: 1, length: 3 };
//!     let partial_data = reader.read_range(&range).await?;
//!     assert_eq!(partial_data.as_slice(), &[2, 3, 4]);
//!
//!     // Reading all data
//!     let all_data = reader.read_all().await?;
//!     assert_eq!(all_data.as_slice(), &[1, 2, 3, 4, 5]);
//!
//!     Ok(())
//! }
//! ```

use crate::{Blob, ByteRange};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;

/// Type alias for a boxed dynamic implementation of the `DataReaderTrait`.
pub type DataReader = Box<dyn DataReaderTrait>;

/// A trait for reading data from various sources.
///
/// # Required Methods
/// - `read_range`: Reads a specific range of bytes from the data source.
/// - `read_all`: Reads all the data from the data source.
/// - `get_name`: Gets the name of the data source.
#[async_trait]
pub trait DataReaderTrait: Debug + Send + Sync {
	/// Reads a specific range of bytes from the data source.
	///
	/// # Arguments
	///
	/// * `range` - A `ByteRange` struct specifying the offset and length of the range to read.
	///
	/// # Returns
	///
	/// * A Result containing a Blob with the read data or an error.
	async fn read_range(&self, range: &ByteRange) -> Result<Blob>;

	/// Reads all the data from the data source.
	///
	/// # Returns
	///
	/// * A Result containing a Blob with all the data or an error.
	#[allow(dead_code)]
	async fn read_all(&self) -> Result<Blob>;

	/// Gets the name of the data source.
	///
	/// # Returns
	///
	/// * A string slice representing the name of the data source.
	fn get_name(&self) -> &str;
}
