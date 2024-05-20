use crate::types::{Blob, ByteRange};
use anyhow::Result;
use async_trait::async_trait;
use byteorder::{BigEndian, LittleEndian};
use std::fmt::Debug;

pub type DataReader = Box<dyn DataReaderTrait>;

#[async_trait]
pub trait DataReaderTrait: Debug + Send + Sync {
	async fn read_range(&mut self, range: &ByteRange) -> Result<Blob>;
	#[allow(dead_code)]
	async fn read_all(&mut self) -> Result<Blob>;
	fn get_name(&self) -> &str;
}
