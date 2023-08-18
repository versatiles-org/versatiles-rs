use super::super::types::ByteRange;
use crate::shared::{Blob, Result};
use async_trait::async_trait;

#[async_trait]
pub trait DataWriterTrait: Send {
	async fn new(filename: &str) -> Result<Box<Self>>
	where
		Self: Sized;
	async fn append(&mut self, blob: &Blob) -> Result<ByteRange>;
	async fn write_start(&mut self, blob: &Blob) -> Result<()>;
	async fn get_position(&mut self) -> Result<u64>;
}
