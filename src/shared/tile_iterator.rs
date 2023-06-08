use super::{Blob, TileCoord3};

type TileItem = (TileCoord3, Blob);
pub type TileIterator = Box<dyn Iterator<Item = TileItem> + Send>;
