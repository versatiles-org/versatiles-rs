use crate::types::{Blob, TileCoord3};
use futures_util::Stream;
use std::pin::Pin;

pub type TilesStream<'a> = Pin<Box<dyn Stream<Item = (TileCoord3, Blob)> + Send + 'a>>;

pub enum ProbeDepth {
	Shallow = 0,
	Container = 1,
	Tiles = 2,
	TileContents = 3,
}
