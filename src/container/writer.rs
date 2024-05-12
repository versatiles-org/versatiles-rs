use super::TilesReaderTrait;
use anyhow::Result;
use async_trait::async_trait;

pub type TilesWriter = Box<dyn TilesWriterTrait>;

#[async_trait]
pub trait TilesWriterTrait: Send {
	// readers must be mutable, because they might use caching
	async fn write_from_reader(&mut self, reader: &mut dyn TilesReaderTrait) -> Result<()>;
}
