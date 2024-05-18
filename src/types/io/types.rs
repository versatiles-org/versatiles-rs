use crate::types::{Blob, ByteRange};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;

pub type DataReader = Box<dyn DataReaderTrait>;

#[async_trait]
pub trait DataReaderTrait: Debug + Send + Sync {
	async fn read_range(&mut self, range: &ByteRange) -> Result<Blob>;
	async fn read_all(&mut self) -> Result<Blob>;
	fn get_name(&self) -> &str;
}

pub trait DataWriterTrait: Send {
	fn append(&mut self, blob: &Blob) -> Result<ByteRange>;
	fn write_start(&mut self, blob: &Blob) -> Result<()>;
	fn get_position(&mut self) -> Result<u64>;
	fn set_position(&mut self, position: u64) -> Result<()>;
}
