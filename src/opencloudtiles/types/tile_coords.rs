#[derive(Eq, PartialEq, Clone, Debug, Hash)]
pub struct TileCoord2 {
	pub x: u64,
	pub y: u64,
}
impl TileCoord2 {
	pub fn new(x: u64, y: u64) -> TileCoord2 {
		TileCoord2 { x, y }
	}
}

#[derive(Eq, PartialEq, Clone, Debug, Hash)]
pub struct TileCoord3 {
	pub x: u64,
	pub y: u64,
	pub z: u64,
}
impl TileCoord3 {
	fn new(x: u64, y: u64, z: u64) -> TileCoord3 {
		TileCoord3 { x, y, z }
	}
}
