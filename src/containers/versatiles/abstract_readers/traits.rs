use super::super::types::ByteRange;
use crate::shared::{Blob, Result};
use async_trait::async_trait;

#[async_trait]
pub trait DataReaderTrait: Send + Sync {
	async fn new(source: &str) -> Result<Box<Self>>
	where
		Self: Sized;
	async fn read_range(&mut self, range: &ByteRange) -> Result<Blob>;
	fn get_name(&self) -> &str;
}
