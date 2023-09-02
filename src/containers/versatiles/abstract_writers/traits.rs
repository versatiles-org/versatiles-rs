use super::super::types::ByteRange;
use crate::shared::{Blob, Result};
use async_trait::async_trait;

#[async_trait]
pub trait DataWriterTrait: Send {
	fn new(filename: &str) -> Result<Box<Self>>
	where
		Self: Sized;
	fn append(&mut self, blob: &Blob) -> Result<ByteRange>;
	fn write_start(&mut self, blob: &Blob) -> Result<()>;
	fn get_position(&mut self) -> Result<u64>;
}
