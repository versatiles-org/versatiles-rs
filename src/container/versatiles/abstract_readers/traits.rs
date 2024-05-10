use super::super::types::ByteRange;
use crate::types::Blob;
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;

#[async_trait]
pub trait DataReaderTrait: Debug + Send + Sync {
	async fn read_range(&mut self, range: &ByteRange) -> Result<Blob>;
	fn get_name(&self) -> &str;
}
