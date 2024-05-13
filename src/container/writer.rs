use super::TilesReader;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait TilesWriter: Send {
	// readers must be mutable, because they might use caching
	async fn write_from_reader(&mut self, reader: &mut dyn TilesReader) -> Result<()>;

	fn boxed(self) -> Box<dyn TilesWriter>
	where
		Self: Sized + 'static,
	{
		Box::new(self)
	}
}
