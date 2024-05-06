use super::TileReaderBox;
use crate::shared::*;
use anyhow::Result;
use async_trait::async_trait;
use futures_util::Stream;
use std::pin::Pin;

#[cfg(feature = "full")]
pub type TileConverterBox = Box<dyn TileConverterTrait>;
pub type TileStream<'a> = Pin<Box<dyn Stream<Item = (TileCoord3, Blob)> + Send + 'a>>;

pub enum ProbeDepth {
	Shallow = 0,
	Container = 1,
	Tiles = 2,
	TileContents = 3,
}

#[allow(clippy::new_ret_no_self)]
#[async_trait]
#[cfg(feature = "full")]
pub trait TileConverterTrait {
	async fn new(filename: &str, tile_config: TileConverterConfig) -> Result<TileConverterBox>
	where
		Self: Sized;

	// readers must be mutable, because they might use caching
	async fn convert_from(&mut self, reader: &mut TileReaderBox) -> Result<()>;
}
