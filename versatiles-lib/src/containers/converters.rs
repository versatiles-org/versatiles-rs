use super::TilesReaderBox;
use crate::shared::TilesConverterConfig;
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

#[cfg(feature = "full")]
pub type TilesConverterBox = Box<dyn TilesConverterTrait>;

#[allow(clippy::new_ret_no_self)]
#[async_trait]
#[cfg(feature = "full")]
pub trait TilesConverterTrait {
	async fn open_file(path: &Path, tile_config: TilesConverterConfig) -> Result<TilesConverterBox>
	where
		Self: Sized;

	// readers must be mutable, because they might use caching
	async fn convert_from(&mut self, reader: &mut TilesReaderBox) -> Result<()>;
}
