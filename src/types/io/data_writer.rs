use crate::types::{Blob, ByteRange};
use anyhow::Result;

pub trait DataWriterTrait: Send {
	fn append(&mut self, blob: &Blob) -> Result<ByteRange>;
	fn write_start(&mut self, blob: &Blob) -> Result<()>;
	fn get_position(&mut self) -> Result<u64>;
	fn set_position(&mut self, position: u64) -> Result<()>;
}
