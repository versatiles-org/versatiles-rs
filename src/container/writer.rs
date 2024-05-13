use super::TilesReaderTrait;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait TilesWriterTrait: Send {
	// readers must be mutable, because they might use caching
	async fn write_from_reader(&mut self, reader: &mut dyn TilesReaderTrait) -> Result<()>;

	fn boxed(self) -> Box<dyn TilesWriterTrait>
	where
		Self: Sized + 'static,
	{
		Box::new(self)
	}
}
