use super::super::types::ByteRange;
use crate::shared::Blob;
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

#[async_trait]
pub trait DataWriterTrait: Send {
	fn new(path: &Path) -> Result<Box<Self>>
	where
		Self: Sized;
	fn append(&mut self, blob: &Blob) -> Result<ByteRange>;
	fn write_start(&mut self, blob: &Blob) -> Result<()>;
	fn get_position(&mut self) -> Result<u64>;
}
